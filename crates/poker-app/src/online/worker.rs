//! Dedicated transport thread. No socket or reconnect waits run on the GUI thread.
use super::{
    commands::OnlineCommand,
    events::{CommandReceipt, PendingKind, WorkerEvent},
    mapping,
};
use poker_client::{Client, ClientConfig, ClientEvent};
use std::sync::mpsc;

pub struct Worker {
    tx: tokio::sync::mpsc::UnboundedSender<OnlineCommand>,
    pub rx: mpsc::Receiver<WorkerEvent>,
}
impl Worker {
    pub fn new(ctx: eframe::egui::Context) -> Self {
        let (tx, mut commands) = tokio::sync::mpsc::unbounded_channel();
        let (events, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let emit = |e| {
                let _ = events.send(e);
                ctx.request_repaint();
            };
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("online runtime");
            runtime.block_on(async {
                let mut client:Option<Client>=None;
                loop {
                    enum Step {Command(Option<OnlineCommand>), Event(Option<ClientEvent>)}
                    let step=if let Some(c)=client.as_mut() {
                        tokio::select! { biased; cmd=commands.recv()=>Step::Command(cmd), event=c.next_event()=>Step::Event(event) }
                    } else {Step::Command(commands.recv().await)};
                    match step {
                        Step::Event(Some(event))=>emit(WorkerEvent::Client(event)),
                        Step::Event(None)=>{client=None;emit(WorkerEvent::WorkerClosed);}
                        Step::Command(None|Some(OnlineCommand::Shutdown))=>break,
                        Step::Command(Some(OnlineCommand::Connect{url,session}))=>{
                            if let Some(mut old)=client.take(){old.disconnect().await;}
                            let mut config=ClientConfig::new(url); config.hello=poker_client::HelloPolicy::compatible_server();
                            match Client::connect(config).await {
                                Ok(mut c)=>{if let Some(session)=session {c.set_session(session);let _=c.resume_session().await;} client=Some(c);}
                                Err(e)=>emit(WorkerEvent::ConnectFailed(e)),
                            }
                        }
                        Step::Command(Some(OnlineCommand::Disconnect))=>{if let Some(mut c)=client.take(){c.clear_session();c.disconnect().await;}emit(WorkerEvent::Client(ClientEvent::Closed));}
                        Step::Command(Some(cmd))=>{
                            let Some(c)=client.as_mut() else {emit(WorkerEvent::CommandFailed{kind:PendingKind::Action,message:"尚未连接服务器".into()});continue;};
                            let mut hand_id=0;let mut turn_id=0;let mut version=-1;
                            let (kind,result)=match cmd {
                                OnlineCommand::CreateRoom{name,options}=>(PendingKind::CreateOrJoin,c.create_room(name,options).await),
                                OnlineCommand::JoinRoom{code,name,options}=>(PendingKind::CreateOrJoin,c.join_room(code,name,options).await),
                                OnlineCommand::StartGame{room_code}=>(PendingKind::StartGame,c.start_game(room_code).await),
                                OnlineCommand::LeaveRoom{room_code}=>{c.clear_session();(PendingKind::LeaveRoom,c.leave_room(room_code).await)},
                                OnlineCommand::ReadyNext{room_code,hand_id:id}=>{hand_id=id;(PendingKind::ReadyNext,c.ready_for_next(room_code,id).await)},
                                OnlineCommand::Action(request)=>{hand_id=request.hand_id;turn_id=request.turn_id;version=request.expected_state_version;(PendingKind::Action,c.player_action(request).await)},
                                OnlineCommand::SetSession(session)=>{c.set_session(session);continue;}
                                OnlineCommand::ClearSession=>{c.clear_session();continue;}
                                OnlineCommand::Reconnect=>{if let Err(e)=c.reconnect().await {emit(WorkerEvent::ConnectFailed(e));}continue;}
                                _=>continue,
                            };
                            match result {Ok(id)=>emit(WorkerEvent::CommandSent(CommandReceipt{request_id:mapping::request_id_key(&id),kind,hand_id,turn_id,expected_state_version:version})),Err(e)=>emit(WorkerEvent::CommandFailed{kind,message:e.to_string()})}
                        }
                    }
                }
                if let Some(mut c)=client {c.disconnect().await;}
                emit(WorkerEvent::WorkerClosed);
            });
        });
        Self { tx, rx }
    }
    pub fn send(&self, cmd: OnlineCommand) {
        let _ = self.tx.send(cmd);
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.send(OnlineCommand::Shutdown);
    }
}
