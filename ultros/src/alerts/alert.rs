use async_trait::async_trait;


/// Alert trait contains the functions required to start or stop an alert
#[async_trait]
trait Alert {
    async fn handle_error();

    async fn destroy();
}

