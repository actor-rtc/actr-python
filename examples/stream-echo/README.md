# Stream Register Example

这是一个简单的 stream 注册和消息传输示例。

## 功能说明

1. **Client** 定义本地服务 `StartStream`（不导出，仅供本地调用）
2. 在 `StartStream` 服务中：
   - 发现 Server
   - 注册 stream 回调 (`ctx.register_stream`)
   - 调用 Server 的 `RegisterStream` RPC
3. **Server** 接收 `RegisterStream` 请求后：
   - 标记 `stream_id`
   - 返回成功响应
   - 启动后台任务向 Client 发送 stream 消息
4. **Client** 通过之前注册的回调处理接收到的 stream 消息

## 目录结构

```
stream-register/
├── server/
│   ├── protos/
│   │   └── stream_server.proto    # Server 服务定义
│   ├── Actr.toml                  # Server 配置
│   ├── Actr.lock.toml            # 依赖锁文件
│   └── server.py                  # Server 实现
│
└── client/
    ├── protos/
    │   └── stream_client.proto    # Client local service definition (LocalStreamService)
    ├── Actr.toml                  # Client configuration
    ├── Actr.lock.toml            # Dependency lock file
    └── client.py                  # Client implementation
```

## 运行示例

### 1. 启动 Server

```bash
cd server
python server.py --actr-toml Actr.toml
```

### 2. 启动 Client

在另一个终端：

```bash
cd client
python client.py --actr-toml Actr.toml 10
```

参数说明：
- `10`: 要接收的 stream 消息数量（stream_id 会自动随机生成8个字符）

## 流程说明

1. Client 启动后，调用自己的 `StartStream` 服务（本地调用），传入消息数量
2. `StartStream` 服务内部：
   - 自动生成 8 位随机 stream_id
   - 发现 Server
   - **先注册** stream 回调
   - 发送 `RegisterStream` 请求给 Server（包含 stream_id 和 message_count）
3. Server 收到请求后：
   - 标记 stream_id
   - 返回成功响应
   - 在后台任务中向 Client 发送 N 条 stream 消息（N = message_count）
4. Client 通过注册的回调接收并处理这些消息

## 关键代码

### Client 端 - StartStream Handler

```python
class LocalStreamService(local_service_actor.LocalStreamServiceHandler):
    async def start_stream(self, req, ctx):
        message_count = req.message_count
        
        # Generate random 8-character stream_id
        stream_id = ''.join(random.choices(string.ascii_lowercase + string.digits, k=8))
        
        # 1. Discover Server
        server_id = await ctx.discover(self.server_type)
        
        # 2. Register callback first
        async def stream_callback(stream: DataStream, sender_id):
            text = stream.payload().decode("utf-8")
            logger.info("Received: %s", text)
        
        await ctx.register_stream(stream_id, stream_callback)
        
        # 3. Call Server's RegisterStream with message_count
        register_req = server_pb2.RegisterStreamRequest(
            stream_id=stream_id,
            message_count=message_count,
        )
        response = await ctx.call(
            Dest.actor(server_id),
            register_req.route_key,
            register_req,
        )
        
        return pb2.StartStreamResponse(success=True, ...)
```

### Client 端 - 主程序

```python
# Create workload with LocalStreamService handler
workload = LocalStreamServiceWorkload(LocalStreamService())
ref = await node.start()

# Call local StartStream service with message_count
start_req = pb2.StartStreamRequest(message_count=args.message_count)
response = await ref.call(start_req.route_key, start_req)
```

### Server 端

```python
async def register_stream(self, req, ctx):
    stream_id = req.stream_id
    message_count = req.message_count
    caller = ctx.caller_id()
    
    # 启动后台任务发送 message_count 条 stream 消息
    async def _send_stream_messages():
        for i in range(1, message_count + 1):
            data_stream = DataStream(
                stream_id=stream_id,
                sequence=i,
                payload=message.encode("utf-8"),
            )
            await ctx.send_stream(Dest.actor(caller), data_stream)
    
    asyncio.create_task(_send_stream_messages())
    
    return RegisterStreamResponse(success=True)
```

## 注意事项

- Client 的 `StartStream` 服务**不在 Actr.toml 的 exports 中**，只供本地调用
- Stream ID 由客户端自动生成（8位随机字符）
- Client 必须在发送 `RegisterStream` 请求**之前**调用 `ctx.register_stream` 注册回调
- Server 根据请求中的 `message_count` 发送相应数量的 stream 消息
- Stream 消息是单向的，不需要响应
