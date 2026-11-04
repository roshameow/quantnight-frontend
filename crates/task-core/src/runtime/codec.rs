//! Bounded physical chunks, no logical response/export total cap.
use tokio::io::{AsyncRead,AsyncWrite,AsyncReadExt,AsyncWriteExt};
use super::protocol::{Message,MAGIC};
pub const CHUNK:usize=64*1024;
pub async fn write_preamble<W:AsyncWrite+Unpin>(out:&mut W)->Result<(),String>{out.write_all(MAGIC).await.map_err(|_|"IPC preamble write failed".into())}
pub async fn read_preamble<R:AsyncRead+Unpin>(input:&mut R)->Result<(),String>{
    let mut magic=[0;8];input.read_exact(&mut magic).await.map_err(|_|"IPC preamble unavailable")?;
    if &magic!=MAGIC{return Err("IPC protocol preamble mismatch".into());}Ok(())
}
pub async fn write<W:AsyncWrite+Unpin>(out:&mut W,message:&Message)->Result<(),String>{
    let bytes=serde_json::to_vec(message).map_err(|_|"IPC message encoding failed")?;
    for part in bytes.chunks(CHUNK){out.write_u32(part.len() as u32).await.map_err(|_|"IPC chunk header write failed")?;out.write_all(part).await.map_err(|_|"IPC chunk write failed")?;}
    out.write_u32(0).await.map_err(|_|"IPC terminator write failed")?;
    out.flush().await.map_err(|_|"IPC flush failed".into())
}
pub async fn read<R:AsyncRead+Unpin>(input:&mut R)->Result<Message,String>{
    let mut bytes=Vec::new();
    loop {
        let length=input.read_u32().await.map_err(|_|"IPC connection ended before message completion")? as usize;
        if length==0{break;}if length>CHUNK{return Err("IPC chunk exceeds physical frame size".into());}
        bytes.try_reserve(length).map_err(|_|"IPC message allocation unavailable")?;
        let start=bytes.len();bytes.resize(start+length,0);
        input.read_exact(&mut bytes[start..]).await.map_err(|_|"IPC connection ended during message")?;
    }
    serde_json::from_slice(&bytes).map_err(|_|"IPC message schema invalid".into())
}

/// Owned large messages serialize through one bounded physical-chunk relay.
/// No second whole JSON Vec, logical-size cap, idle deadline or business quota.
/// Dropping the transport receiver unblocks serialization, never its original core operation.
pub async fn write_owned<W:AsyncWrite+Unpin>(out:&mut W,message:Message)->Result<(),String>{
    use std::io::Write;
    struct Relay {sender:tokio::sync::mpsc::Sender<Vec<u8>>,buffer:Vec<u8>}
    impl Relay {
        fn emit(&mut self)->std::io::Result<()>{
            if self.buffer.is_empty(){return Ok(());}
            let bytes=std::mem::replace(&mut self.buffer,Vec::with_capacity(CHUNK));
            self.sender.blocking_send(bytes).map_err(|_|std::io::Error::new(std::io::ErrorKind::BrokenPipe,"IPC transport ended"))
        }
    }
    impl Write for Relay {
        fn write(&mut self,mut bytes:&[u8])->std::io::Result<usize>{let total=bytes.len();while !bytes.is_empty(){let n=(CHUNK-self.buffer.len()).min(bytes.len());self.buffer.extend_from_slice(&bytes[..n]);bytes=&bytes[n..];if self.buffer.len()==CHUNK{self.emit()?;}}Ok(total)}
        fn flush(&mut self)->std::io::Result<()>{self.emit()}
    }
    let(sender,mut receiver)=tokio::sync::mpsc::channel(1);
    let serializer=tokio::task::spawn_blocking(move||{
        let mut relay=Relay{sender,buffer:Vec::with_capacity(CHUNK)};
        serde_json::to_writer(&mut relay,&message).map_err(|_|"IPC message encoding failed".to_string())?;
        relay.flush().map_err(|_|"IPC serialization transport ended".to_string())
    });
    while let Some(bytes)=receiver.recv().await{
        out.write_u32(bytes.len() as u32).await.map_err(|_|"IPC chunk header write failed")?;
        out.write_all(&bytes).await.map_err(|_|"IPC chunk write failed")?;
    }
    serializer.await.map_err(|_|"IPC serializer interrupted")??;
    out.write_u32(0).await.map_err(|_|"IPC terminator write failed")?;
    out.flush().await.map_err(|_|"IPC flush failed".into())
}
