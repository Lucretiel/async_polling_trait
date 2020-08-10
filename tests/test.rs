use polling_async_trait::async_poll_trait;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

#[async_poll_trait]
trait ExampleTrait {
    // This will create an async method called `basic` on this trait
    #[async_method]
    fn poll_basic(&mut self, cx: &mut Context<'_>) -> Poll<i32>;

    // polling methods can also accept &self or Pin<&mut Self>
    #[async_method]
    fn poll_ref_method(&self, cx: &mut Context<'_>) -> Poll<i32>;

    #[async_method]
    fn poll_pin_method(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<i32>;

    // If `owned` is given, the generated async method will take `self` by move.
    // This means that the returned future will take ownership of this instance.
    // Owning futures can still be used with any of `&self`, `&mut self`, or
    // `Pin<&mut Self>`
    #[async_method(owned)]
    fn poll_close(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>>;

    #[async_method(owned)]
    fn poll_close_ref(&self, cx: &mut Context<'_>) -> Poll<io::Result<()>>;

    #[async_method(owned)]
    fn poll_close_pinned(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>>;

    // you can use method_name and future_name to control the names of the
    // generated async method and associated future. This will generate an
    // async method called do_work, and an associated `Future` called `DoWork`
    #[async_method(method_name = "do_work", future_name = "DoWork")]
    fn poll_work(&mut self, cx: &mut Context<'_>) -> Poll<()>;
}

#[derive(Default)]
struct ExampleStruct {
    closed: bool,
}

impl ExampleTrait for ExampleStruct {
    fn poll_basic(&mut self, _cx: &mut Context<'_>) -> Poll<i32> {
        Poll::Ready(10)
    }

    fn poll_ref_method(&self, _cx: &mut Context<'_>) -> Poll<i32> {
        Poll::Ready(20)
    }

    fn poll_pin_method(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<i32> {
        Poll::Ready(30)
    }

    fn poll_close(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if !self.closed {
            println!("closing...");
            self.closed = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        } else {
            println!("closed!");
            Poll::Ready(Ok(()))
        }
    }
    fn poll_close_ref(&self, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if !self.closed {
            println!("Error, couldn't close...");
            Poll::Ready(Err(io::ErrorKind::Other.into()))
        } else {
            println!("closed!");
            Poll::Ready(Ok(()))
        }
    }

    fn poll_close_pinned(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if !this.closed {
            println!("closing...");
            this.closed = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        } else {
            println!("closed!");
            Poll::Ready(Ok(()))
        }
    }

    fn poll_work(&mut self, _cx: &mut Context<'_>) -> Poll<()> {
        Poll::Ready(())
    }
}

#[tokio::test]
async fn test_all() -> io::Result<()> {
    let mut data1 = ExampleStruct::default();

    assert_eq!(data1.basic().await, 10);
    assert_eq!(data1.ref_method().await, 20);
    data1.do_work().await;
    data1.close().await?;

    let data2 = ExampleStruct::default();
    assert!(data2.close_ref().await.is_err());

    let mut data3 = Box::pin(ExampleStruct::default());
    assert_eq!(data3.as_mut().pin_method().await, 30);

    let data4 = ExampleStruct::default();

    // Soundness: we can can await this method directly because it takes
    // ownership of `data4`.
    data4.close_pinned().await?;

    Ok(())
}
