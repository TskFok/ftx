import React, { useCallback } from "react";
import { Button, Tooltip, message } from "antd";
import { UploadOutlined, DownloadOutlined } from "@ant-design/icons";
import { invoke } from "@tauri-apps/api/core";
import { useFileBrowserStore } from "../../stores/fileBrowserStore";
import { useTransferStore } from "../../stores/transferStore";
import { useOverwriteStore } from "../../stores/overwriteStore";
import { isConnectionError } from "../../utils/connectionError";

const TransferActionBar: React.FC = () => {
  const {
    localFiles,
    remoteFiles,
    selectedLocalFiles,
    selectedRemoteFiles,
    connectedHostId,
    remotePath,
    localPath,
    clearConnectionState,
  } = useFileBrowserStore();
  const startUpload = useTransferStore((s) => s.startUpload);
  const startDownload = useTransferStore((s) => s.startDownload);
  const startDirectoryUpload = useTransferStore((s) => s.startDirectoryUpload);
  const startDirectoryDownload = useTransferStore(
    (s) => s.startDirectoryDownload,
  );
  const showDialog = useOverwriteStore((s) => s.showDialog);
  const resetOverwriteAll = useOverwriteStore((s) => s.resetOverwriteAll);

  const handleConnectionError = useCallback(
    (err: unknown) => {
      if (isConnectionError(err)) {
        clearConnectionState();
      }
    },
    [clearConnectionState],
  );

  const handleUpload = useCallback(async () => {
    if (!connectedHostId) {
      message.warning("请先连接主机");
      return;
    }
    if (selectedLocalFiles.length === 0) {
      message.warning("请在左侧选择要上传的文件或目录");
      return;
    }

    const selectedEntries = localFiles.filter((f) =>
      selectedLocalFiles.includes(f.path),
    );
    const filesToUpload = selectedEntries.filter((f) => !f.is_dir);
    const dirsToUpload = selectedEntries.filter((f) => f.is_dir);

    if (filesToUpload.length === 0 && dirsToUpload.length === 0) {
      message.warning("请选择要上传的文件或目录");
      return;
    }

    resetOverwriteAll();

    for (const dir of dirsToUpload) {
      const remoteTargetDir = remotePath.endsWith("/")
        ? `${remotePath}${dir.name}`
        : `${remotePath}/${dir.name}`;
      try {
        const ids = await startDirectoryUpload(
          connectedHostId,
          dir.path,
          remoteTargetDir,
        );
        message.info(`目录 ${dir.name} 已加入上传队列（${ids.length} 个文件）`);
      } catch (err) {
        handleConnectionError(err);
        message.error(`上传目录 ${dir.name} 失败: ${err}`);
      }
    }

    for (const file of filesToUpload) {
      const remoteFilePath = remotePath.endsWith("/")
        ? `${remotePath}${file.name}`
        : `${remotePath}/${file.name}`;

      try {
        const exists = await invoke<boolean>("remote_file_exists", {
          hostId: connectedHostId,
          path: remoteFilePath,
        });

        if (exists) {
          const action = await showDialog({
            hostId: connectedHostId,
            localPath: file.path,
            remotePath: remoteFilePath,
            filename: file.name,
            fileSize: file.size,
            direction: "upload",
          });

          if (action === "skip") continue;
          if (action === "rename") {
            const ext = file.name.includes(".")
              ? "." + file.name.split(".").pop()
              : "";
            const base = file.name.replace(/\.[^.]+$/, "");
            const newName = `${base}_${Date.now()}${ext}`;
            const newRemotePath = remotePath.endsWith("/")
              ? `${remotePath}${newName}`
              : `${remotePath}/${newName}`;
            await startUpload(
              connectedHostId,
              file.path,
              newRemotePath,
              newName,
              file.size,
            );
            continue;
          }
        }

        await startUpload(
          connectedHostId,
          file.path,
          remoteFilePath,
          file.name,
          file.size,
        );
      } catch (err) {
        handleConnectionError(err);
        message.error(`上传 ${file.name} 失败: ${err}`);
      }
    }
  }, [
    connectedHostId,
    handleConnectionError,
    selectedLocalFiles,
    localFiles,
    remotePath,
    startUpload,
    startDirectoryUpload,
    showDialog,
    resetOverwriteAll,
  ]);

  const handleDownload = useCallback(async () => {
    if (!connectedHostId) {
      message.warning("请先连接主机");
      return;
    }
    if (selectedRemoteFiles.length === 0) {
      message.warning("请在右侧选择要下载的文件或目录");
      return;
    }

    const selectedEntries = remoteFiles.filter((f) =>
      selectedRemoteFiles.includes(f.path),
    );
    const filesToDownload = selectedEntries.filter((f) => !f.is_dir);
    const dirsToDownload = selectedEntries.filter((f) => f.is_dir);

    if (filesToDownload.length === 0 && dirsToDownload.length === 0) {
      message.warning("请选择要下载的文件或目录");
      return;
    }

    resetOverwriteAll();

    const targetDir = localPath || "/tmp";

    for (const dir of dirsToDownload) {
      const localTargetDir = targetDir.endsWith("/")
        ? `${targetDir}${dir.name}`
        : `${targetDir}/${dir.name}`;
      try {
        const ids = await startDirectoryDownload(
          connectedHostId,
          dir.path,
          localTargetDir,
        );
        message.info(`目录 ${dir.name} 已加入下载队列（${ids.length} 个文件）`);
      } catch (err) {
        handleConnectionError(err);
        message.error(`下载目录 ${dir.name} 失败: ${err}`);
      }
    }

    for (const file of filesToDownload) {
      const localFilePath = targetDir.endsWith("/")
        ? `${targetDir}${file.name}`
        : `${targetDir}/${file.name}`;

      try {
        const exists = await invoke<boolean>("check_local_file_exists", {
          path: localFilePath,
        });

        if (exists) {
          const action = await showDialog({
            hostId: connectedHostId,
            localPath: localFilePath,
            remotePath: file.path,
            filename: file.name,
            fileSize: file.size,
            direction: "download",
          });

          if (action === "skip") continue;
          if (action === "rename") {
            const ext = file.name.includes(".")
              ? "." + file.name.split(".").pop()
              : "";
            const base = file.name.replace(/\.[^.]+$/, "");
            const newName = `${base}_${Date.now()}${ext}`;
            const newLocalPath = targetDir.endsWith("/")
              ? `${targetDir}${newName}`
              : `${targetDir}/${newName}`;
            await startDownload(
              connectedHostId,
              file.path,
              newLocalPath,
              newName,
              file.size,
            );
            continue;
          }
        }

        await startDownload(
          connectedHostId,
          file.path,
          localFilePath,
          file.name,
          file.size,
        );
      } catch (err) {
        handleConnectionError(err);
        message.error(`下载 ${file.name} 失败: ${err}`);
      }
    }
  }, [
    connectedHostId,
    handleConnectionError,
    selectedRemoteFiles,
    remoteFiles,
    localPath,
    startDownload,
    startDirectoryDownload,
    showDialog,
    resetOverwriteAll,
  ]);

  const hasLocalSelection = selectedLocalFiles.length > 0;
  const hasRemoteSelection = selectedRemoteFiles.length > 0;

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        justifyContent: "center",
        alignItems: "center",
        gap: 12,
        padding: "0 4px",
      }}
    >
      <Tooltip title="上传选中文件/目录到远程">
        <Button
          type="primary"
          icon={<UploadOutlined />}
          onClick={handleUpload}
          disabled={!hasLocalSelection || !connectedHostId}
          size="small"
        />
      </Tooltip>
      <Tooltip title="下载选中文件/目录到本地">
        <Button
          icon={<DownloadOutlined />}
          onClick={handleDownload}
          disabled={!hasRemoteSelection || !connectedHostId}
          size="small"
        />
      </Tooltip>
    </div>
  );
};

export default TransferActionBar;
