use std::fs::{File};
use std::{io, thread};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Stdio};
use anyhow::{Context};
use serde::{Deserialize, Serialize};

fn main() -> anyhow::Result<()>{
    simple_logger::init_with_level(log::Level::Info)?;
    println!("Dump IQ Server Start");
    let listener = TcpListener::bind("0.0.0.0:9600")?;
    println!("Waiting for connect...");
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                thread::spawn(|| {
                    let _ = handle_client(stream);
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
    CopyFiles(String),
    SetReg{
        addr: u32,
        value: u32
    },
    ShellCmd(String),
    ATEInit,
    ATECmd(String)
}

#[derive(Serialize, Deserialize, Debug)]
struct  ResponseHeader {
    is_error: bool,
    file_size: u64
}

fn handle_client(mut stream: TcpStream) -> anyhow::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);

    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let request: DumpCommand = serde_json::from_str(&line)
            .with_context(|| {
                send_response(&mut stream, true, 0);
                "Json Parse Error"
            })?;
        log::info!("Receive Command {:?}", request);

        match request {
            DumpCommand::DumpIQ { band_5g, file_name } => {
                let res = dump_iq(band_5g, &file_name)?;
                send_response(&mut stream, !res, 0);
            }
            DumpCommand::DelFiles => {
                let output = Command::new("/bin/ash")
                    .arg("-c")
                    .arg("rm -rf /tmp/*.txt")
                    .status()?;
                log::info!("Delete file over");
                send_response(&mut stream, !output.success(), 0)
            }
            DumpCommand::CopyFiles(s) => {
                match File::open(format!("/tmp/{}", s)) {
                    Ok(mut file) => {
                        let metadata = file.metadata()?;
                        let size = metadata.len();
                        send_response(&mut stream, false, size);
                        io::copy(&mut file, &mut stream)?;
                        stream.flush()?;
                        log::info!("Copy file {} Over", &s)
                    }
                    Err(_e) => {
                        log::error!("Can open file {}", s);
                        send_response(&mut stream, true, 0);
                    }
                }
            }
            DumpCommand::SetReg { addr, value } => {
                let _output = Command::new("devmem")
                    .arg(format!("0x{:08X}", addr))
                    .arg("32")
                    .arg(format!("0x{:08X}", value))
                    .status()?;
                log::info!("devmem {} 32 {}", format!("0x{:08X}", addr), format!("0x{:08X}", value));
            }
            DumpCommand::ShellCmd(s) => {
                shell_cmd(&s)?;
                log::info!("Shell Cmd {}", s);
            }
            DumpCommand::ATEInit => {
                ate_init()?;
                log::info!("ATE_init Over!")
            }
            // DumpCommand::OpenRx { band_5g } => {
            //     todo!();
            //     let cmd = if band_5g {
            //         ["wlan0", "fastconfig", "-f", "5745", "-c", "5745", "-w", "1", "-r"]
            //     } else {
            //         [ "wlan1", "fastconfig", "-f", "2422", "-c", "2422", "-w", "0", "-r"]
            //     };
            //     let _ = Command::new("ate_cmd")
            //         .args(cmd)
            //         .status()?;
            // }
            DumpCommand::ATECmd(s) => {
                let cmd = s
                    .trim()
                    .split(" ")
                    .collect::<Vec<_>>();
                let _ = Command::new("ate_cmd")
                    .args(cmd)
                    .status()?;
                log::info!("ate_cmd {} over", s);
            }
        }
    }
}

fn send_response(stream: &mut TcpStream, is_error: bool, file_size: u64) {
    let header = ResponseHeader {
        is_error,
        file_size
    };
    let json = serde_json::to_string(&header).unwrap();
    stream.write_all(json.as_bytes()).unwrap();
    stream.write_all(b"\n").unwrap();
}

fn ate_init() -> anyhow::Result<()> {
    let _ = Command::new("iw")
        .args(["phy", "phy1", "interface", "add", "wlan0", "type", "managed"])
        .status()?;
    let _ = Command::new("iw")
        .args(["phy", "phy0", "interface", "add", "wlan1", "type", "managed"])
        .status()?;
    let _ = Command::new("ifconfig")
        .arg("wlan0")
        .arg("up")
        .status()?;
    let _ = Command::new("ifconfig")
        .arg("wlan1")
        .arg("up")
        .status()?;
    Ok(())
}

fn shell_cmd(cmd: &str) -> anyhow::Result<()> {
    let _output = Command::new("/bin/ash")
        .arg("-c")
        .arg(cmd)
        .status()?;
    Ok(())
}

fn dump_iq(is_5g: bool, file_name: &str) -> anyhow::Result<bool>{
    if is_5g {
        shell_cmd("echo 0 1 0 15 0 e000 0 2 0  1 0 0 0 > /sys/kernel/debug/ieee80211/phy1/siwifi/iq_engine")?;

        let memdump = Command::new("memdump")
            .arg("0x20000000")
            .arg("0x62000")
            .stdout(Stdio::piped())
            .spawn()
            .expect("fail to spawn memdump");
        let hexdump = Command::new("hexdump")
            .args(["-v", "-e", r#""0x%08x""\n""#])
            .stdin(memdump.stdout.unwrap())
            .stdout(Stdio::piped())
            .spawn()
            .expect("failed to spawn hexdump");
        let output = hexdump
            .wait_with_output()
            .expect("failed to read hexdump output");
        let mut file = File::create(format!("/tmp/{}", file_name))?;
        file.write_all(&output.stdout)?;

        log::info!("Dump Over");
        // Ok(output2.success() & output1.success())
        Ok(true)
    } else {
        shell_cmd("echo 0 1 0 15 0 1c000 0 2 0  1 0 0 0 > /sys/kernel/debug/ieee80211/phy0/siwifi/iq_engine")?;

        let memdump = Command::new("memdump")
            .arg("0x30000000")
            .arg("0xd8000")
            .stdout(Stdio::piped())
            .spawn()
            .expect("fail to spawn memdump");
        let hexdump = Command::new("hexdump")
            .args(["-v", "-e", r#""0x%08x""\n""#])
            .stdin(memdump.stdout.unwrap())
            .stdout(Stdio::piped())
            .spawn()
            .expect("failed to spawn hexdump");
        let output = hexdump
            .wait_with_output()
            .expect("failed to read hexdump output");
        let mut file = File::create(format!("/tmp/{}", file_name))?;
        file.write_all(&output.stdout)?;

        log::info!("Dump Over");
        // Ok(output2.success() & output1.success())
        Ok(true)
    }
}