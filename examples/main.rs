use polling_async_trait::async_poll_trait;
use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};
use tokio;

#[async_poll_trait]
trait SimpleAsync {
    #[async_method]
    fn poll_flush(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>>;

    #[async_method(owned)]
    fn poll_close(self: Pin<&mut Self>, cs: &mut Context<'_>) -> Poll<io::Result<()>>;
}

struct TestStruct {
    flushed: bool,
    closed: bool,
}

impl SimpleAsync for TestStruct {
    fn poll_flush(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.closed {
            Poll::Ready(Result::Err(io::ErrorKind::NotConnected.into()))
        } else if self.flushed {
            println!("Finished the flush!");
            Poll::Ready(Ok(()))
        } else {
            println!("Beginning to flush...");
            self.flushed = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.closed {
            println!("Completed the close!");
            Poll::Ready(Ok(()))
        } else {
            println!("Beginning to close...");
            self.get_mut().closed = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let mut thing = TestStruct {
        flushed: false,
        closed: false,
    };

    thing.flush().await?;
    thing.close().await?;

    Ok(())
}
