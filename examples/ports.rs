//! List MIDI ports and mark the pedal's: `cargo run --example ports`.

use midir::{MidiInput, MidiOutput};
use open_stomp::is_stomp_port;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input = MidiInput::new("open-stomp ports")?;
    let output = MidiOutput::new("open-stomp ports")?;
    println!("inputs:");
    for p in input.ports() {
        let name = input.port_name(&p)?;
        println!("  {} {name}", if is_stomp_port(&name) { "*" } else { " " });
    }
    println!("outputs:");
    for p in output.ports() {
        let name = output.port_name(&p)?;
        println!("  {} {name}", if is_stomp_port(&name) { "*" } else { " " });
    }
    Ok(())
}
