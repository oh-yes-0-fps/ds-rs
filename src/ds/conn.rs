use super::state::{State, Mode};
use super::Signal;

use crate::util::*;
use crate::inbound::udp::UdpResponsePacket;
use crate::inbound::tcp::*;
use crate::outbound::udp::types::tags::{*, DateTime as DTTag};
use crate::outbound::tcp::tags::*;

use std::net::{UdpSocket, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Instant, Duration};
use std::io::{self, Write, Read};
use std::thread;

use chrono::prelude::*;
use crossbeam_channel::{self, Receiver, Sender};
use byteorder::{ReadBytesExt, BigEndian};

pub fn udp_thread(state: Arc<Mutex<State>>, tx: Sender<Signal>, rx: Receiver<Signal>, team_number: u32) {
    let mut tcp_connected = false;
    let target_ip = ip_from_team_number(team_number);
    let mut last = Instant::now();
//            let udp_tx = UdpSocket::bind("10.40.69.1:5678").unwrap();
    let udp_tx = UdpSocket::bind("10.40.69.65:5678").unwrap();
    udp_tx.connect(&format!("{}:1110", target_ip)).unwrap();

//            let udp_rx = UdpSocket::bind("10.40.69.1:1150").unwrap();
    let udp_rx = UdpSocket::bind("10.40.69.65:1150").unwrap();
    udp_rx.set_nonblocking(true).unwrap();

    println!("UDP sockets open.");

    loop {
        match rx.try_recv() {
            Ok(Signal::Disconnect) | Err(crossbeam_channel::TryRecvError::Disconnected) => break,
            _ => {}
        }

        // Buffer to hold the upcoming packet from the roborio
        let mut buf = [0u8; 100];

        match udp_rx.recv_from(&mut buf[..]) {
            Ok(_) => {
                let mut state = state.lock().unwrap();
                if let Ok(packet) = UdpResponsePacket::decode(&buf[..], state.seqnum()) {

                    // if need_date is set, the roborio expects DateTime and Timezone tags on the following heartbeat
                    if packet.need_date {
                        let local = Utc::now();
                        let micros = local.naive_utc().timestamp_subsec_micros();
                        let second = local.time().second() as u8;
                        let minute = local.time().minute() as u8;
                        let hour = local.time().hour() as u8;
                        let day = local.date().day() as u8;
                        let month = local.date().month0() as u8;
                        let year = (local.date().year() - 1900) as u8;
                        let tag = DTTag::new(micros, second, minute, hour, day, month, year);
                        state.queue(TagType::DateTime(tag));

                        let tz = Timezone::new("Canada/Eastern");
                        state.queue(TagType::Timezone(tz));
                    }
                    // Update the state for the next iteration
                    let mode = Mode::from_status(packet.status).unwrap();
                    state.set_mode(mode);
                    state.increment_seqnum();
                    if !tcp_connected {
                        tcp_connected = true;
                        tx.try_send(Signal::ConnectTcp).unwrap();
                    }
                }
            }
            Err(e) => {
                if e.kind() != io::ErrorKind::WouldBlock {
                    panic!("{}", e);
                }
            }
        }

        // roboRIO packets should be >=20ms apart, once there should send control packet
        if last.elapsed() >= Duration::from_millis(20) {
            let mut state = state.lock().unwrap();
            last = Instant::now();
            udp_tx.send(&state.control().encode()[..]).unwrap();
        }

        thread::sleep(Duration::from_millis(20));
    }
}

pub fn tcp_thread(state: Arc<Mutex<State>>, rx: Receiver<Signal>, team_number: u32) {
    let target_ip = ip_from_team_number(team_number);

    match rx.recv() {
        Ok(Signal::Disconnect) | Err(_) => return,
        _ => {}
    }

    let mut conn = TcpStream::connect(&format!("{}:1740", target_ip)).unwrap();
    conn.set_read_timeout(Some(Duration::from_secs(2)));

    loop {
        match rx.try_recv() {
            Ok(Signal::Disconnect) | Err(crossbeam_channel::TryRecvError::Disconnected) => break,
            _ => {}
        }

        // Nested scope because otherwise we could deadlock on `state` if TCP doesn't get anything before the next UDP packet needs to be sent
        {
            let mut state = state.lock().unwrap();
            if !state.pending_tcp().is_empty() {
                for tag in state.pending_tcp() {
                    match tag {
                        TcpTag::GameData(gd) => conn.write(&gd.construct()[..]).unwrap(),
                        TcpTag::MatchInfo(mi) => conn.write(&mi.construct()[..]).unwrap(),
                    };
                }
            }
        }


        let mut prelim = [0; 2];
        if let Ok(_) = conn.read(&mut prelim) {
            // prelim will hold the size of the incoming packet at this point
            let mut prelim = &prelim[..];
            let size = prelim.read_u16::<BigEndian>().unwrap();

            // At this point buf will hold the entire packet minus length prefix.
            let mut buf = vec![0; size as usize];
            conn.read(&mut buf[..]).unwrap();

            match buf[0] {
                // stdout
                0x0c => if let Ok(stdout) = Stdout::decode(&buf[..]) {
                    println!("{}", stdout.message);
                }
                _ => {}
            }
        }
    }
}