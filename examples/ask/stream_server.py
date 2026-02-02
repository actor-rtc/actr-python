# DO NOT EDIT - Generated scaffold
# TODO: Implement your business logic

from __future__ import annotations

import argparse
import asyncio
import logging
import sys
from pathlib import Path

from actr import ActrSystem, WorkloadBase, Context

# 配置日志
logging.basicConfig(
    level=logging.INFO,
    format="[%(levelname)s] %(message)s",
)
logger = logging.getLogger(__name__)

# 添加 generated 目录到 Python 路径
generated_dir = Path(__file__).parent / "generated"
if str(generated_dir) not in sys.path:
    sys.path.insert(0, str(generated_dir))

# 动态导入生成的模块
from generated.local import ask_service_pb2 as pb2
from generated import stream_server_service_actor as actor
from actr import DataStream, Dest
import uuid


class StreamServerHandler(actor.StreamServerHandler):
    """
    StreamServer 业务逻辑实现
    TODO: 在此类中实现具体的 RPC 方法
    """

    def __init__(self) -> None:
        logger.info("StreamServerHandler 实例已初始化")

    async def usr_prompt(
        self, req: pb2.UsrPromptRequest, ctx: Context
    ) -> pb2.AssistantReply:
        """
        实现 UsrPrompt RPC 方法
        1. 提取请求中的 voice_stream_id（如果有）
        2. 注册 data_stream 回调来接收语音数据
        3. 生成并返回答案的 stream_id

        Args:
            req: UsrPromptRequest 请求对象
            ctx: Actor 上下文

        Returns:
            AssistantReply 响应对象，包含答案的 stream_id
        """
        logger.info("📝 接收到 UsrPrompt 请求")
        logger.info(f"   Question ID: {req.question_id}")
        logger.info(f"   Session ID: {req.session_id}")
        logger.info(f"   Text: {req.text}")
        logger.info(f"   Voice Stream ID: {req.voice_stream_id}")
        
        caller = ctx.caller_id()
        if caller is None:
            logger.error("❌ 无法获取 caller_id")
            return pb2.AssistantReply(
                question_id=req.question_id,
                session_id=req.session_id,
                text="",
                stream_id="",
                status_code=1,
                error_message="No caller_id in context"
            )
        
        # 如果请求中包含 voice_stream_id，注册回调来接收语音数据并原样返回
        if req.voice_stream_id:
            logger.info(f"🎤 注册语音流回调: {req.voice_stream_id}")
            
            # 获取目标（调用者）
            target = Dest.actor(caller)
            
            async def voice_stream_callback(stream: DataStream, sender_id):
                """处理接收到的语音流数据 - 原样返回"""
                logger.info(f"🔊 收到语音数据: stream_id={stream.stream_id()}, sequence={stream.sequence()}, size={len(stream.payload())} bytes")
                
                # 创建返回的数据流，使用 answer_stream_id
                echo_stream = DataStream(
                    stream_id=answer_stream_id,
                    sequence=stream.sequence(),  # 保持相同的序列号
                    payload=stream.payload(),     # 原样返回数据
                )
                
                try:
                    # 将接收到的数据原样发送回调用者
                    await ctx.send_stream(target, echo_stream)
                    logger.info(f"  📤 已回传数据: sequence={stream.sequence()}, size={len(stream.payload())} bytes"")
                except Exception as e:
                    logger.error(f"  ❌ 回传数据失败: {e}")
            
            try:
                await ctx.register_stream(req.voice_stream_id, voice_stream_callback)
                logger.info(f"✅ 语音流回调注册成功: {req.voice_stream_id}")
            except Exception as e:
                logger.error(f"❌ 注册语音流回调失败: {e}")
        
        # 生成答案的 stream_id
        answer_stream_id = f"answer-{req.question_id}-{uuid.uuid4().hex[:8]}"
        logger.info(f"🆔 生成答案流 ID: {answer_stream_id}")
        
        # 如果没有 voice_stream_id，发送默认的答案流
        if not req.voice_stream_id:
            # 启动异步任务发送答案流
            async def send_answer_stream():
                """发送流式答案数据"""
                logger.info(f"🌊 开始发送答案流: {answer_stream_id}")
                
                # 模拟 AI 生成的答案片段
                answer_chunks = [
                    "根据您的问题，",
                    "我理解您想了解",
                    f"关于 '{req.text}' 的信息。",
                    "让我为您详细解答：",
                    "首先，这个问题涉及到...",
                    "其次，我们需要考虑...",
                    "最后，建议您...",
                    "希望这个回答对您有帮助！"
                ]
                
                target = Dest.actor(caller)
                
                for i, chunk in enumerate(answer_chunks, 1):
                    data_stream = DataStream(
                        stream_id=answer_stream_id,
                        sequence=i,
                        payload=chunk.encode('utf-8'),
                    )
                    
                    try:
                        await ctx.send_stream(target, data_stream)
                        logger.info(f"  📤 发送答案片段 {i}/{len(answer_chunks)}: {chunk}")
                        await asyncio.sleep(0.3)  # 模拟流式输出延迟
                    except Exception as e:
                        logger.error(f"  ❌ 发送答案片段失败: {e}")
                        break
                
                logger.info(f"✅ 答案流发送完成: {answer_stream_id}")
            
            # 启动异步任务（不等待完成）
            asyncio.create_task(send_answer_stream())
        
        # 立即返回响应，告诉客户端答案的 stream_id
        return pb2.AssistantReply(
            question_id=req.question_id,
            session_id=req.session_id,
            text="",  # 实际内容通过 stream 发送
            stream_id=answer_stream_id,
            status_code=0,
            error_message=""
        )

    async def unregister_data_stream(
        self, req: pb2.UnregisterRequest, ctx: Context
    ) -> pb2.UnregisterResponse:
        """
        实现 UnregisterDataStream RPC 方法 - 注销数据流

        Args:
            req: UnregisterRequest 请求对象
            ctx: Actor 上下文

        Returns:
            UnregisterResponse 响应对象
        """
        logger.info("🔓 接收到 UnregisterDataStream 请求")
        logger.info(f"   Stream ID: {req.stream_id}")
        
        try:
            # 注销数据流回调
            await ctx.unregister_stream(req.stream_id)
            logger.info(f"✅ 数据流注销成功: {req.stream_id}")
            
            return pb2.UnregisterResponse(
                success=True,
                message=f"Stream {req.stream_id} unregistered successfully"
            )
        except Exception as e:
            logger.error(f"❌ 数据流注销失败: {e}")
            return pb2.UnregisterResponse(
                success=False,
                message=f"Failed to unregister stream: {str(e)}"
            )

    async def attach(self, req: pb2.AttachRequest, ctx: Context) -> pb2.AttachResponse:
        """
        实现 Attach RPC 方法 - 处理附件上传

        Args:
            req: AttachRequest 请求对象
            ctx: Actor 上下文

        Returns:
            AttachResponse 响应对象
        """
        logger.info("📎 接收到附件上传请求")
        logger.info(f"   ID: {req.id}")
        logger.info(f"   文件名: {req.filename}")
        logger.info(f"   类型: {pb2.AttachmentType.Name(req.type)}")
        logger.info(f"   大小: {len(req.data)} bytes")
        
        # TODO: 这里可以实现实际的附件存储逻辑
        # 例如：保存到本地文件系统或云存储
        # 示例：
        # attachment_path = f"attachments/{req.id}_{req.filename}"
        # with open(attachment_path, 'wb') as f:
        #     f.write(req.data)
        
        return pb2.AttachResponse(
            id=req.id,
            status_code=0,
            error_message=""
        )


class StreamServerWorkload(WorkloadBase):
    def __init__(self, handler: StreamServerHandler):
        self.handler = handler
        super().__init__(actor.StreamServerDispatcher())

    async def on_start(self, ctx: Context) -> None:
        logger.info("🚀 工作负载 StreamServerWorkload 正在启动...")

    async def on_stop(self, ctx: Context) -> None:
        logger.info("🛑 工作负载 StreamServerWorkload 正在停止...")


async def main() -> int:
    ap = argparse.ArgumentParser(description="StreamServer Runner")
    ap.add_argument("--actr-toml", required=True, help="ACTR 配置文件路径")
    args = ap.parse_args()

    logger.info("🔧 正在初始化 StreamServer 系统...")
    system = await ActrSystem.from_toml(args.actr_toml)

    workload = StreamServerWorkload(StreamServerHandler())

    node = system.attach(workload)
    ref = await node.start()

    logger.info("✅ StreamServer 启动成功! Actor ID: %s", ref.actor_id())

    # 等待中断信号并关闭
    await ref.wait_for_ctrl_c_and_shutdown()
    logger.info("👋 StreamServer 已关闭")

    return 0


if __name__ == "__main__":
    try:
        sys_exit_code = asyncio.run(main())
        raise SystemExit(sys_exit_code)
    except KeyboardInterrupt:
        pass
