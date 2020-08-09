use polling_traits::async_poll_trait;
use std::{
    io,
    task::{Context, Poll},
};
use tokio;

#[async_poll_trait]
trait SimpleAsync {
    /*
    #[async_ref]
    fn poll_flush(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>>;
    */

    #[async_owned]
    fn poll_close(&mut self, cs: &mut Context<'_>) -> Poll<io::Result<()>>;
}

struct TestStruct {
    flushed: bool,
    closed: bool,
}

impl SimpleAsync for TestStruct {
    /*
    fn poll_flush(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.closed {
            Poll::Ready(Result::Err(io::ErrorKind::NotConnected.into()))
        } else if self.flushed {
            Poll::Ready(Ok(()))
        } else {
            self.flushed = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
    */

    fn poll_close(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.closed {
            println!("Completed the close!");
            Poll::Ready(Ok(()))
        } else {
            println!("Beginning to close...");
            self.closed = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let thing = TestStruct {
        flushed: false,
        closed: false,
    };

    //thing.flush().await?;
    thing.close().await?;

    Ok(())
}
