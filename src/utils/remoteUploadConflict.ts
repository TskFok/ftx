import type { FileEntry } from "../types";

/** 当前远端列表中是否已有同名「文件」（不含目录），用于补充后端 exists 检测的空窗。 */
export function remoteDirListsFileNamed(
  remoteFiles: FileEntry[],
  fileName: string,
): boolean {
  return remoteFiles.some((f) => !f.is_dir && f.name === fileName);
}
