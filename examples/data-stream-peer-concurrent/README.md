# Data Stream Peer Concurrent Example (Python)

This example demonstrates bidirectional peer-to-peer streaming communication with concurrent support:

```
actr_ref -> local StreamClient handler -> (ctx.call, ctx.send_data_stream) -> remote StreamServer handler -> register_stream
```

## Features

- ✅ **Bidirectional communication**: Both client and server can send DataStream messages
- ✅ **Concurrent support**: Server handles multiple clients concurrently
- ✅ **Peer-to-peer**: Uses actr_ref for actor discovery and communication

## Flow

1. The client app uses `actr_ref.call()` to invoke `StreamClient.StartStream` on the local workload.
2. The local handler discovers a `DataStreamPeerConcurrentServer` actor and calls `StreamServer.PrepareStream`.
3. The server handler calls `StreamClient.PrepareClientStream` so the client registers a DataStream callback.
4. The server sends DataStream chunks back to the client.
5. The client handler sends DataStream chunks with `ctx.send_data_stream()`.

## Proto

`proto/data_stream_peer.proto` defines two services:

- `StreamClient.StartStream`
- `StreamClient.PrepareClientStream`
- `StreamServer.PrepareStream`

## Setup

### Option 1: Use start.sh script (Recommended)

The `start.sh` script will automatically:
- Create a Python virtual environment
- Install dependencies
- Build and install actr_sdk
- Generate protobuf code
- Start actrix (signaling server)
- Set up realms
- Start server and client

```bash
cd actr-python/examples/data-stream-peer-concurrent
./start.sh [--client-id ID] [--count N]
```

### Option 2: Manual Setup

1. Install actr_sdk:

```bash
cd actr-python  # Go to actr-python root
maturin develop --release
```

2. Generate Python protobuf code:

```bash
cd examples/data-stream-peer-concurrent

# Create generated directory
mkdir -p generated/actr
touch generated/__init__.py
touch generated/actr/__init__.py

# Generate data_stream_peer protobuf
protoc -I proto --python_out=generated proto/data_stream_peer.proto

# Generate actr protocol protobuf (for DataStream)
ACTR_PROTO_DIR="../../../actr/crates/protocol/proto"
protoc -I "$ACTR_PROTO_DIR" --python_out=generated \
  "$ACTR_PROTO_DIR/actr/options.proto" \
  "$ACTR_PROTO_DIR/actr.proto" \
  "$ACTR_PROTO_DIR/package.proto"
```

## Run

1. Start the signaling server (actrix):

```bash
# From actrix directory
cargo run -- --config ../actr-examples/actrix-config.example.toml
```

2. Start the server:

```bash
cd server
python server.py --actr-toml Actr.toml
```

3. Run the client:

```bash
cd client
python client.py --actr-toml Actr.toml client-1 5
```

## Notes

- The Python SDK uses decorators (`@actr.service`, `@actr.rpc`) to define services
- DataStream callbacks are registered with `ctx.register_stream()`
- DataStream messages are sent with `ctx.send_stream()`
- The high-level API automatically handles serialization/deserialization

