use anyhow::bail;
use std::fs;
use std::io::Write;
use std::process::Command;
use std::thread;
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};

fn start() -> anyhow::Result<()> {
    let mut stdout = StandardStream::stdout(ColorChoice::Always);

    stdout.set_color(ColorSpec::new().set_fg(Some(Color::Yellow)))?;
    writeln!(&mut stdout, "Starting rustkrazy")?;

    for service in fs::read_dir("/bin")? {
        let service = service?;
        let service_name = match service.file_name().into_string() {
            Ok(v) => v,
            Err(_) => bail!("[ ERROR ] invalid unicode in file name"),
        };

        if service_name == "init" {
            continue;
        }

        match Command::new(service.path()).spawn() {
            Ok(_) => {
                stdout.set_color(ColorSpec::new().set_fg(Some(Color::Green)))?;
                writeln!(&mut stdout, "[  OK   ] Starting {}", service_name)?;
                stdout.set_color(ColorSpec::new().set_fg(Some(Color::White)))?;
            }
            Err(e) => {
                stdout.set_color(ColorSpec::new().set_fg(Some(Color::Red)))?;
                writeln!(&mut stdout, "[ ERROR ] Starting {}: {}", service_name, e)?;
                stdout.set_color(ColorSpec::new().set_fg(Some(Color::White)))?;
            }
        }
    }

    Ok(())
}

fn main() {
    match start() {
        Ok(_) => {}
        Err(e) => {
            let mut stdout = StandardStream::stdout(ColorChoice::Always);

            match stdout.set_color(ColorSpec::new().set_fg(Some(Color::Red))) {
                Ok(_) => match writeln!(&mut stdout, "[ ERROR ] {}", e) {
                    Ok(_) => {}
                    Err(_) => println!("[ ERROR ] {}", e),
                },
                Err(_) => {
                    println!("[ ERROR ] {}", e);
                }
            }
        }
    }

    loop {
        thread::yield_now();
    }
}
