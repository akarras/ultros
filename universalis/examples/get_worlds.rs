use universalis::UniversalisClient;

// Just print out all worlds
#[tokio::main]
async fn main() {
    println!(
        "{:?}",
        UniversalisClient::new("ultros-universalis-examples")
            .get_worlds()
            .await
            .unwrap()
    );
}
