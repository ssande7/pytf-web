use std::{time::{Duration, Instant}, sync::Arc};

use actix::prelude::*;
use actix_web_actors::ws;

use crate::job_queue::{Job, JobServer, WorkerConnect, WorkerDisconnect};

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);

const WORKER_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Debug)]
pub struct WorkerWsSession {
    pub id: usize,

    pub heartbeat: Instant,

    pub job: Option<Job>,

    pub addr: Addr<JobServer>,
}

impl WorkerWsSession {
    fn heartbeat(&self, ctx: &mut ws::WebsocketContext<Self>) {
        ctx.run_interval(HEARTBEAT_INTERVAL, |act, ctx| {
            if Instant::now().duration_since(act.heartbeat) > WORKER_TIMEOUT {
                println!("Lost connection to client {}", act.id);
                act.addr.do_send(WorkerDisconnect { id: act.id });
                ctx.stop();
                return;
            }
            ctx.ping(b"");
        });
    }
}


impl Actor for WorkerWsSession {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        self.heartbeat(ctx);

        let addr = ctx.address();
        self.addr
            .send(WorkerConnect { addr })
            .into_actor(self)
            .then(|res, act, ctx| {
                match res {
                    Ok(res) => act.id = res,
                    _ => ctx.stop(), // Something went wrong
                }
                fut::ready(())
            })
            .wait(ctx);
    }
}
