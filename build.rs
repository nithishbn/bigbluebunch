fn main() {
    // Compile the GTFS-realtime protobuf definition
    prost_build::compile_protos(&["proto/gtfs-realtime.proto"], &["proto/"])
        .expect("Failed to compile protobuf definitions");
}
