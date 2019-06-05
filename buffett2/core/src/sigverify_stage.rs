use buffett_metrics::counter::Counter;
use influx_db_client as influxdb;
use log::Level;
use buffett_metrics::metrics;
use crate::packet::SharedPackets;
use rand::{thread_rng, Rng};
use crate::result::{Error, Result};
use crate::service::Service;
use crate::sigverify;
use std::sync::atomic::AtomicUsize;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, spawn, JoinHandle};
use std::time::Instant;
use crate::streamer::{self, PacketReceiver};
use buffett_timing::timing;

pub type VerifiedPackets = Vec<(SharedPackets, Vec<u8>)>;

pub struct SigVerifyStage {
    thread_hdls: Vec<JoinHandle<()>>,
}

impl SigVerifyStage {
    pub fn new(
        packet_receiver: Receiver<SharedPackets>,
        sigverify_disabled: bool,
    ) -> (Self, Receiver<VerifiedPackets>) {
        sigverify::init();
        let (verified_sender, verified_receiver) = channel();
        let thread_hdls =
            Self::verifier_services(packet_receiver, verified_sender, sigverify_disabled);
        (SigVerifyStage { thread_hdls }, verified_receiver)
    }

    fn verify_batch(batch: Vec<SharedPackets>, sigverify_disabled: bool) -> VerifiedPackets {
        let r = if sigverify_disabled {
            sigverify::ed25519_verify_disabled(&batch)
        } else {
            sigverify::ed25519_verify(&batch)
        };
        batch.into_iter().zip(r).collect()
    }

    fn verifier(
        recvr: &Arc<Mutex<PacketReceiver>>,
        sendr: &Arc<Mutex<Sender<VerifiedPackets>>>,
        sigverify_disabled: bool,
    ) -> Result<()> {
        let (batch, len, recv_time) =
            streamer::recv_batch(&recvr.lock().expect("'recvr' lock in fn verifier"))?;
        sub_new_counter_info!("sigverify_stage-entries_received", len);

        let now = Instant::now();
        let batch_len = batch.len();
        let rand_id = thread_rng().gen_range(0, 100);
        info!(
            "@{:?} verifier: verifying: {} id: {}",
            timing::timestamp(),
            batch.len(),
            rand_id
        );

        let verified_batch = Self::verify_batch(batch, sigverify_disabled);
        sub_new_counter_info!(
            "sigverify_stage-verified_entries_send",
            verified_batch.len()
        );

        if sendr
            .lock()
            .expect("lock in fn verify_batch in tpu")
            .send(verified_batch)
            .is_err()
        {
            return Err(Error::SendError);
        }

        let total_time_ms = timing::duration_in_milliseconds(&now.elapsed());
        let total_time_s = timing::duration_in_seconds(&now.elapsed());
        sub_new_counter_info!(
            "sigverify_stage-time_ms",
            (total_time_ms + recv_time) as usize
        );
        info!(
            "@{:?} verifier: done. batches: {} total verify time: {:?} id: {} verified: {} v/s {}",
            timing::timestamp(),
            batch_len,
            total_time_ms,
            rand_id,
            len,
            (len as f32 / total_time_s)
        );

        metrics::submit(
            influxdb::Point::new("sigverify_stage-total_verify_time")
                .add_field("batch_len", influxdb::Value::Integer(batch_len as i64))
                .add_field("len", influxdb::Value::Integer(len as i64))
                .add_field(
                    "total_time_ms",
                    influxdb::Value::Integer(total_time_ms as i64),
                ).to_owned(),
        );

        Ok(())
    }

    fn verifier_service(
        packet_receiver: Arc<Mutex<PacketReceiver>>,
        verified_sender: Arc<Mutex<Sender<VerifiedPackets>>>,
        sigverify_disabled: bool,
    ) -> JoinHandle<()> {
        spawn(move || loop {
            if let Err(e) = Self::verifier(&packet_receiver, &verified_sender, sigverify_disabled) {
                match e {
                    Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => break,
                    Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                    Error::SendError => {
                        break;
                    }
                    _ => error!("{:?}", e),
                }
            }
        })
    }

    fn verifier_services(
        packet_receiver: PacketReceiver,
        verified_sender: Sender<VerifiedPackets>,
        sigverify_disabled: bool,
    ) -> Vec<JoinHandle<()>> {
        let sender = Arc::new(Mutex::new(verified_sender));
        let receiver = Arc::new(Mutex::new(packet_receiver));
        (0..4)
            .map(|_| Self::verifier_service(receiver.clone(), sender.clone(), sigverify_disabled))
            .collect()
    }
}

impl Service for SigVerifyStage {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        for thread_hdl in self.thread_hdls {
            thread_hdl.join()?;
        }
        Ok(())
    }
}
