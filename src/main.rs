use std::alloc::System;
use std::fs::{File, FileTimes};
use std::{io, thread};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Command;
use anyhow::Context;
use serde::{Deserialize, Serialize};

fn main() -> anyhow::Result<()>{
    println!("Dump IQ Server Start");
    let listener = TcpListener::bind("0.0.0.0:9600")?;
    println!("Waiting for connect...");
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                thread::spawn(|| {
                    handle_client(stream);
                });
            }
            Err(e) => {
                log::error!("Connect Error:{}", e)
            }
        }
    }
    Ok(())
}

#[derive(Serialize, Deserialize, Debug)]
enum DumpCommand {
    DumpIQ{
        band_5g: bool,
        file_name: String
    },
    DelFiles,
    CopyFiles(String)
}

#[derive(Serialize, Deserialize, Debug)]
struct  ResponseHeader {
    is_error: bool
}

fn handle_client(mut stream: TcpStream) -> anyhow::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();

    reader.read_line(&mut line)?;
    let request: DumpCommand = serde_json::from_str(&line)
        .with_context(|| {
            send_response(&mut stream, true);
            "Json Parse Error"
        })?;
    log::info!("Receive Command {:?}", request);

    match request {
        DumpCommand::DumpIQ { band_5g, file_name } => {
            let res = dump_iq(band_5g, &file_name)?;
            send_response(&mut stream, !res);
        }
        DumpCommand::DelFiles => {
            let output = Command::new("rm")
                .arg("-rf")
                .arg("/tmp/*.txt")
                .status()?;
            send_response(&mut stream, !output.success())
        }
        DumpCommand::CopyFiles(s) => {
            match File::open(&s) {
                Ok(mut file) => {
                    let metadata = file.metadata()?;
                    send_response(&mut stream, false);
                    io::copy(&mut file, &mut stream)?;
                }
                Err(e) => {
                    log::error!("Can open file {}", s);
                    send_response(&mut stream, true);
                }
            }
        }
    }


    Ok(())
}

fn send_response(stream: &mut TcpStream, is_error: bool) {
    let header = ResponseHeader {
        is_error
    };
    let json = serde_json::to_string(&header).unwrap();
    stream.write_all(json.as_bytes()).unwrap();
    stream.write_all(b"\n").unwrap();
}

fn dump_iq(is_5g: bool, file_name: &str) -> anyhow::Result<bool>{
    if is_5g {
        let output1 = Command::new("echo")
            .arg("0 1 0 15 0 e000 0 2 0  1 0 0 0 > /sys/kernel/debug/ieee80211/phy1/siwifi/iq_engine")
            .status()?;
        let output2 = Command::new("memdump")
            .arg(" 0x20000000 0x62000 | hexdump  -v -e \'\"0x%08x\"\"\n\"\'")
            .arg(format!("> /tmp/{}", file_name))
            .status()?;
        Ok(output2.success() & output1.success())
    } else {
        let output1 = Command::new("echo")
            .arg("0 1 0 15 0 1c000 0 2 0  1 0 0 0 > /sys/kernel/debug/ieee80211/phy0/siwifi/iq_engine")
            .status()?;
        let output2 = Command::new("memdump")
            .arg(" 0x30000000 0xd8000 | hexdump  -v -e \'\"0x%08x\"\"\n\"\'")
            .arg(format!("> /tmp/{}", file_name))
            .status()?;
        Ok(output2.success() & output1.success())
    }
}