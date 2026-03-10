/**
 * 判断错误是否为连接断开类错误（如空闲超时、连接不存在等）
 */
export function isConnectionError(err: unknown): boolean {
  const msg = String(err ?? "");
  return (
    msg.includes("idle timeout") ||
    msg.includes("No active connection") ||
    msg.includes("Connection closed")
  );
}
