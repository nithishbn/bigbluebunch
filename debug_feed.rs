use prost::Message;

// Include the generated protobuf code
pub mod gtfs_realtime {
    include!(concat!(env!("OUT_DIR"), "/transit_realtime.rs"));
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let url = "http://gtfs.bigbluebus.com/vehiclepositions.bin";

    println!("Fetching from: {}", url);
    let response = reqwest::get(url).await?;
    let bytes = response.bytes().await?;

    println!("Received {} bytes", bytes.len());

    let feed = gtfs_realtime::FeedMessage::decode(&bytes[..])?;

    println!("Feed header version: {:?}", feed.header.gtfs_realtime_version);
    println!("Feed timestamp: {:?}", feed.header.timestamp);
    println!("Number of entities: {}", feed.entity.len());

    for (i, entity) in feed.entity.iter().enumerate() {
        println!("\n--- Entity {} ---", i);
        println!("Entity ID: {}", entity.id);

        if let Some(vehicle) = &entity.vehicle {
            println!("Has vehicle data: YES");

            if let Some(trip) = &vehicle.trip {
                println!("  Trip ID: {:?}", trip.trip_id);
                println!("  Route ID: {:?}", trip.route_id);
                println!("  Direction ID: {:?}", trip.direction_id);
            } else {
                println!("  Trip data: NONE");
            }

            if let Some(veh) = &vehicle.vehicle {
                println!("  Vehicle ID: {:?}", veh.id);
                println!("  Vehicle label: {:?}", veh.label);
            } else {
                println!("  Vehicle descriptor: NONE");
            }

            if let Some(pos) = &vehicle.position {
                println!("  Position: {}, {}", pos.latitude, pos.longitude);
                println!("  Bearing: {:?}", pos.bearing);
                println!("  Speed: {:?}", pos.speed);
            } else {
                println!("  Position: NONE");
            }

            println!("  Timestamp: {:?}", vehicle.timestamp);
            println!("  Current stop: {:?}", vehicle.current_stop_sequence);
        } else {
            println!("Has vehicle data: NO");
        }
    }

    Ok(())
}
