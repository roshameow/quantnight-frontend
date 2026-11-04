//! One original task watcher per runtime, fanout only; no task lifecycle policy.
use std::sync::Arc;
use tokio::sync::{Mutex,broadcast};
use super::{TaskRuntimeContext,RuntimeEvent,TransportError,RuntimeObserver,ProgressSubscription};
struct State{users:usize,handle:Option<crate::task_progress::ProgressHandle>}
pub(crate) struct ProgressHub{context:Arc<TaskRuntimeContext>,state:Mutex<State>,events:broadcast::Sender<RuntimeEvent>}
pub(crate) struct Lease(Arc<ProgressHub>);
impl Drop for Lease {fn drop(&mut self){let hub=self.0.clone();tokio::spawn(async move{let mut state=hub.state.lock().await;state.users-=1;if state.users==0{if let Some(handle)=state.handle.take(){handle.shutdown().await;}}});}}
impl ProgressHub {
    pub(crate) fn new(context:Arc<TaskRuntimeContext>)->Arc<Self>{let(events,_)=broadcast::channel(256);Arc::new(Self{context,state:Mutex::new(State{users:0,handle:None}),events})}
    pub(crate) async fn subscribe(self:&Arc<Self>)->Result<(broadcast::Receiver<RuntimeEvent>,Lease),TransportError>{
        let mut state=self.state.lock().await;let receiver=self.events.subscribe();
        if state.handle.is_none(){let sender=self.events.clone();let sink:crate::task_progress::ProgressSink=Arc::new(move|name,payload|{let _=sender.send(RuntimeEvent::GuiProgress{name:name.into(),payload});});
            state.handle=Some(crate::task_progress::start(self.context.clone(),sink).await.map_err(|e|TransportError::new("application","progress",e))?);
        }
        state.users+=1;if let Some(handle)=&state.handle{handle.refresh();}
        Ok((receiver,Lease(self.clone())))
    }
    pub(crate) async fn refresh(&self){if let Some(handle)=&self.state.lock().await.handle{handle.refresh();}}
    pub(crate) async fn client_subscription(self:&Arc<Self>,sink:RuntimeObserver)->Result<ProgressSubscription,TransportError>{
        let(mut rx,lease)=self.subscribe().await?;
        Ok(ProgressSubscription{task:Some(tokio::spawn(async move{let _lease=lease;loop{match rx.recv().await{Ok(event)=>sink(event),Err(broadcast::error::RecvError::Lagged(dropped))=>sink(RuntimeEvent::ProgressGap{dropped}),Err(_)=>break}}}))})
    }
}
