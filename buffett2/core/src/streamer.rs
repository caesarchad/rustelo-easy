use influx_db_client as influxdb;
use crate::metrics;
use crate::packet::{Blob, SharedBlobs, SharedPackets};
use crate::result::{Error, Result};
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::{Builder, JoinHandle};
use std::time::{Duration, Instant};
use buffett_timing::timing::duration_in_milliseconds;

pub type PacketReceiver = Receiver<SharedPackets>;
pub type PacketSender = Sender<SharedPackets>;
pub type BlobSender = Sender<SharedBlobs>;
pub type BlobReceiver = Receiver<SharedBlobs>;

fn recv_loop(
    sock: &UdpSocket,
    exit: &Arc<AtomicBool>,
    channel: &PacketSender,
    channel_tag: &'static str,
) -> Result<()> {
    loop {
        let msgs = SharedPackets::default();
        loop {
            if exit.load(Ordering::Relaxed) {
                return Ok(());
            }
            if msgs.write().unwrap().recv_from(sock).is_ok() {
                let len = msgs.read().unwrap().packets.len();
                metrics::submit(
                    influxdb::Point::new(channel_tag)
                        .add_field("count", influxdb::Value::Integer(len as i64))
                        .to_owned(),
                );
                channel.send(msgs)?;
                break;
            }
        }
    }
}

pub fn receiver(
    sock: Arc<UdpSocket>,
    exit: Arc<AtomicBool>,
    packet_sender: PacketSender,
    sender_tag: &'static str,
) -> JoinHandle<()> {
    let res = sock.set_read_timeout(Some(Duration::new(1, 0)));
    if res.is_err() {
        panic!("streamer::receiver set_read_timeout error");
    }
    Builder::new()
        .name("bitconch-receiver".to_string())
        .spawn(move || {
            let _ = recv_loop(&sock, &exit, &packet_sender, sender_tag);
            ()
        }).unwrap()
}

fn recv_send(sock: &UdpSocket, r: &BlobReceiver) -> Result<()> {
    let timer = Duration::new(1, 0);
    let msgs = r.recv_timeout(timer)?;
    Blob::send_to(sock, msgs)?;
    Ok(())
}

pub fn recv_batch(recvr: &PacketReceiver) -> Result<(Vec<SharedPackets>, usize, u64)> {
    let timer = Duration::new(1, 0);
    let msgs = recvr.recv_timeout(timer)?;
    let recv_start = Instant::now();
    trace!("got msgs");
    let mut len = msgs.read().unwrap().packets.len();
    let mut batch = vec![msgs];
    while let Ok(more) = recvr.try_recv() {
        trace!("got more msgs");
        len += more.read().unwrap().packets.len();
        batch.push(more);

        if len > 100_000 {
            break;
        }
    }
    trace!("batch len {}", batch.len());
    Ok((batch, len, duration_in_milliseconds(&recv_start.elapsed())))
}

pub fn responder(name: &'static str, sock: Arc<UdpSocket>, r: BlobReceiver) -> JoinHandle<()> {
    Builder::new()
        .name(format!("bitconch-responder-{}", name))
        .spawn(move || loop {
            if let Err(e) = recv_send(&sock, &r) {
                match e {
                    Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => break,
                    Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                    _ => warn!("{} responder error: {:?}", name, e),
                }
            }
        }).unwrap()
}


fn recv_blobs(sock: &UdpSocket, s: &BlobSender) -> Result<()> {
    trace!("recv_blobs: receiving on {}", sock.local_addr().unwrap());
    let dq = Blob::recv_from(sock)?;
    if !dq.is_empty() {
        s.send(dq)?;
    }
    Ok(())
}

pub fn blob_receiver(sock: Arc<UdpSocket>, exit: Arc<AtomicBool>, s: BlobSender) -> JoinHandle<()> {
    
    let timer = Duration::new(1, 0);
    sock.set_read_timeout(Some(timer))
        .expect("set socket timeout");
    Builder::new()
        .name("bitconch-blob_receiver".to_string())
        .spawn(move || loop {
            if exit.load(Ordering::Relaxed) {
                break;
            }
            let _ = recv_blobs(&sock, &s);
        }).unwrap()
}

