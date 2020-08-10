# polling-async-trait

`polling-async-trait` is a library that creates async methods associated with polling methods on your traits. It is similar to [`async-trait`], but where `async-trait` works on `async` methods, `polling-async-trait` works on `poll_` methods.

## Usage

The entry point to this library is the `async_poll_trait` attribute. When applied to a trait, it scans the trait for each method tagged with `async_method`. It treats each of these methods as an async polling method, and for each one, it adds an equivalent async method to the trait.

```rust
use polling_async_trait::async_poll_trait;
use std::io;

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
    fn poll_close_pinned(self: Pin<&mut Self>, cx: &mut Context<'_>)
        -> Poll<io::Result<()>>;

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
    fn poll_basic(&mut self, cx: &mut Context<'_>) -> Poll<i32> {
        Poll::Ready(10)
    }

    fn poll_ref_method(&self, cx: &mut Context<'_>) -> Poll<i32> {
        Poll::Ready(20)
    }

    fn poll_pin_method(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<i32> {
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

    fn poll_close_ref(&self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
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

    fn poll_work(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        Poll::Ready(())
    }
}

#[tokio::main]
async fn main() -> io::Result<()> {
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
```

The generated future types will share visibility with the trait (that is, they will be `pub` if the trait is `pub`, `pub(crate)` if the trait is `pub(crate)`, etc).

## Tradeoffs with [`async-trait`]

Consider carefully which library is best for your use case; polling methods are often much more difficult to write (because they require manual state management & dealing with `Pin`). If your control flow is complex, it's probably preferable to use an `async fn` and [`async-trait`]. The advantage of `polling-async-trait` is that the async methods it creates are 0-overhead, because the returned futures call the poll methods directly. This means there's no need to use a type-erased `Box<dyn Future ... >`.

[`async-trait`]: https://docs.rs/async-trait

License: MPL-2.0
