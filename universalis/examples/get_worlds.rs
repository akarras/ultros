use universalis::UniversalisClient;

// Just print out all worlds
#[tokio::main]
async fn main() {
    println!("{:?}", UniversalisClient::new().get_worlds().await.unwrap());
}
