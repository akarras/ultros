use crate::event::{EventProducer, EventType};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};
use ultros_api_types::websocket::{ListingEventData, SaleEventData};
use ultros_db::UltrosDb;
use universalis::websocket::SocketRx;
use universalis::websocket::event_types::{EventChannel, SubscribeMode, WSMessage};
use universalis::{WebsocketClient, WorldId};

pub(crate) async fn run_socket_listener(
    db: UltrosDb,
    listings_tx: EventProducer<ListingEventData>,
    sales_tx: EventProducer<SaleEventData>,
    token: CancellationToken,
) {
    let mut socket = WebsocketClient::connect().await;
    socket
        .update_subscription(SubscribeMode::Subscribe, EventChannel::ListingsAdd, None)
        .await;
    socket
        .update_subscription(SubscribeMode::Subscribe, EventChannel::ListingsRemove, None)
        .await;
    socket
        .update_subscription(SubscribeMode::Subscribe, EventChannel::SalesAdd, None)
        .await;
    let receiver = socket.get_receiver();
    loop {
        tokio::select! {
            _ = token.cancelled() => {
                info!("socket listener cancelled");
                break;
            }
            msg = receiver.recv() => {
                if let Some(msg) = msg {
                    // create a new task for each message
                    let db = db.clone();
            // hopefully this is cheap to clone
            let listings_tx = listings_tx.clone();
            let sales_tx = sales_tx.clone();
            if let SocketRx::Event(Ok(e)) = &msg {
                let world_id = WorldId::from(e);
                metrics::counter!("ultros_websocket_rx", "WorldId" => world_id.0.to_string())
                    .increment(1);
            }
            tokio::spawn(async move {
                let db = &db;
                match msg {
                    SocketRx::Event(Ok(WSMessage::ListingsAdd {
                        item,
                        world,
                        listings,
                    })) => match db.update_listings(listings.clone(), item, world).await {
                        Ok((listings, removed)) => {
                            let listings = Arc::new(ListingEventData {
                                item_id: item.0,
                                world_id: world.0,
                                listings,
                            });
                            let removed = Arc::new(ListingEventData {
                                item_id: item.0,
                                world_id: world.0,
                                listings: removed,
                            });
                            match listings_tx.send(EventType::Remove(removed)) {
                                Ok(o) => info!(slack_remaining = o, "sent removed listings"),
                                Err(e) => error!(error = ?e, "Error removing listings"),
                            }
                            match listings_tx.send(EventType::Add(listings)) {
                                Ok(o) => info!(remaining_slack = o, "updated listings"),
                                Err(e) => error!(error = ?e, "Error adding listings"),
                            };
                        }
                        Err(e) => error!(error = ?e, listings = ?listings, "Listing add failed"),
                    },
                    SocketRx::Event(Ok(WSMessage::ListingsRemove {
                        item,
                        world,
                        listings,
                    })) => match db.remove_listings(listings.clone(), item, world).await {
                        Ok(listings) => {
                            info!(?listings, ?item, ?world, "Removed listings");
                            if let Err(e) = listings_tx.send(EventType::removed(ListingEventData {
                                item_id: item.0,
                                world_id: world.0,
                                listings,
                            })) {
                                error!(error = ?e, "Error sending remove listings");
                            }
                        }
                        Err(e) => {
                            error!(error = ?e, ?listings, ?item, ?world, "Error removing listings. Listings set")
                        }
                    },
                    SocketRx::Event(Ok(WSMessage::SalesAdd { item, world, sales })) => {
                        match db.update_sales(sales.clone(), item, world).await {
                            Ok(added_sales) => {
                                info!(?added_sales, ?item, ?world, "Stored sale data");
                                match sales_tx
                                    .send(EventType::added(SaleEventData { sales: added_sales }))
                                {
                                    Ok(o) => info!(slack_remaining = o, "Sent sale"),
                                    Err(e) => error!(error = ?e, "Error sending sale update"),
                                }
                            }
                            Err(e) => {
                                error!(error = ?e, ?sales, ?item, ?world, "Error inserting sale.")
                            }
                        }
                    }
                    SocketRx::Event(Ok(WSMessage::SalesRemove { item, world, sales })) => {
                        info!(?item, ?world, ?sales, "sales removed");
                    }
                    SocketRx::Event(Err(e)) => {
                        error!(error = ?e, "Error");
                    }
                }
            });
                }
            }
        }
    }
}
