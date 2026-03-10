import React, { useCallback, useState } from "react";
import { Button, Card, Space, Input, Modal, message, Popconfirm, Tooltip } from "antd";
import {
  ArrowUpOutlined,
  ReloadOutlined,
  FolderAddOutlined,
  DeleteOutlined,
  StarOutlined,
} from "@ant-design/icons";
import { invoke } from "@tauri-apps/api/core";
import type { FileEntry } from "../../types";
import PathBreadcrumb from "./PathBreadcrumb";
import FileTable from "./FileTable";
import { useFileBrowserStore } from "../../stores/fileBrowserStore";
import { isConnectionError } from "../../utils/connectionError";

interface FilePanelProps {
  title: string;
  mode: "local" | "remote";
  path: string;
  files: FileEntry[];
  loading: boolean;
  selectedFiles: string[];
  hostId?: number | null;
  onNavigate: (path: string) => void;
  onNavigateUp: () => void;
  onRefresh: () => void;
  onSelect: (paths: string[]) => void;
  onAddBookmark?: (hostId: number, path: string) => void;
}

const FilePanel: React.FC<FilePanelProps> = ({
  title,
  mode,
  path,
  files,
  loading,
  selectedFiles,
  hostId,
  onNavigate,
  onNavigateUp,
  onRefresh,
  onSelect,
  onAddBookmark,
}) => {
  const [mkdirVisible, setMkdirVisible] = useState(false);
  const [newDirName, setNewDirName] = useState("");
  const clearConnectionState = useFileBrowserStore(
    (s) => s.clearConnectionState,
  );

  const handleConnectionError = useCallback(
    (err: unknown) => {
      if (isConnectionError(err)) {
        clearConnectionState();
      }
    },
    [clearConnectionState],
  );

  const handleCreateDir = useCallback(async () => {
    if (!newDirName.trim()) return;
    const newPath = path.endsWith("/")
      ? `${path}${newDirName}`
      : `${path}/${newDirName}`;
    try {
      if (mode === "remote" && hostId) {
        await invoke("create_remote_dir", { hostId, path: newPath });
      } else if (mode === "local") {
        await invoke("list_local_dir", { path: newPath }).catch(async () => {
          // If directory doesn't exist, the Rust backend doesn't support mkdir for local
          // We'll handle this gracefully
          throw new Error("本地目录创建暂不支持");
        });
      }
      message.success("文件夹创建成功");
      setMkdirVisible(false);
      setNewDirName("");
      onRefresh();
    } catch (err) {
      handleConnectionError(err);
      message.error(`创建失败: ${err}`);
    }
  }, [newDirName, path, mode, hostId, onRefresh, handleConnectionError]);

  const handleDelete = useCallback(
    async (file: FileEntry) => {
      try {
        if (mode === "remote" && hostId) {
          if (file.is_dir) {
            await invoke("delete_remote_dir", { hostId, path: file.path });
          } else {
            await invoke("delete_remote_file", { hostId, path: file.path });
          }
        } else {
          throw new Error("本地文件删除暂不支持");
        }
        message.success(`已删除 ${file.name}`);
        onRefresh();
      } catch (err) {
        handleConnectionError(err);
        message.error(`删除失败: ${err}`);
      }
    },
    [mode, hostId, onRefresh, handleConnectionError],
  );

  const handleRename = useCallback(
    async (file: FileEntry) => {
      const newName = window.prompt("请输入新名称", file.name);
      if (!newName || newName === file.name) return;
      const parentDir = file.path.replace(/\/[^/]+\/?$/, "") || "/";
      const newPath = parentDir.endsWith("/")
        ? `${parentDir}${newName}`
        : `${parentDir}/${newName}`;
      try {
        if (mode === "remote" && hostId) {
          await invoke("rename_remote", {
            hostId,
            from: file.path,
            to: newPath,
          });
        } else {
          throw new Error("本地重命名暂不支持");
        }
        message.success("重命名成功");
        onRefresh();
      } catch (err) {
        handleConnectionError(err);
        message.error(`重命名失败: ${err}`);
      }
    },
    [mode, hostId, onRefresh, handleConnectionError],
  );

  const handleBatchDelete = useCallback(async () => {
    const selected = files.filter((f) => selectedFiles.includes(f.path));
    for (const file of selected) {
      await handleDelete(file);
    }
    onSelect([]);
  }, [files, selectedFiles, handleDelete, onSelect]);

  const disabled = mode === "remote" && !hostId;

  return (
    <Card
      title={title}
      size="small"
      style={{ height: "100%", display: "flex", flexDirection: "column" }}
      styles={{ body: { flex: 1, overflow: "hidden", padding: "8px 12px" } }}
      extra={
        <Space size={4}>
          <Button
            type="text"
            size="small"
            icon={<ArrowUpOutlined />}
            onClick={onNavigateUp}
            disabled={disabled || path === "/"}
            title="上级目录"
          />
          <Button
            type="text"
            size="small"
            icon={<ReloadOutlined />}
            onClick={onRefresh}
            disabled={disabled}
            title="刷新"
          />
          {mode === "remote" && (
            <Button
              type="text"
              size="small"
              icon={<FolderAddOutlined />}
              onClick={() => setMkdirVisible(true)}
              disabled={disabled}
              title="新建文件夹"
            />
          )}
          {mode === "remote" && hostId && onAddBookmark && (
            <Tooltip title="收藏此目录">
              <Button
                type="text"
                size="small"
                icon={<StarOutlined />}
                disabled={disabled}
                onClick={(e) => {
                  e.stopPropagation();
                  onAddBookmark(hostId, path || "/");
                }}
              />
            </Tooltip>
          )}
          {selectedFiles.length > 0 && mode === "remote" && (
            <Popconfirm
              title={`确认删除 ${selectedFiles.length} 个项目？`}
              onConfirm={handleBatchDelete}
              okText="删除"
              cancelText="取消"
              okButtonProps={{ danger: true }}
            >
              <Button
                type="text"
                size="small"
                danger
                icon={<DeleteOutlined />}
                title="删除选中"
              />
            </Popconfirm>
          )}
        </Space>
      }
    >
      <div style={{ marginBottom: 8 }}>
        <PathBreadcrumb path={path || "/"} onNavigate={onNavigate} />
      </div>

      {disabled ? (
        <div
          style={{
            color: "#999",
            textAlign: "center",
            padding: "40px 0",
          }}
        >
          请先连接主机
        </div>
      ) : (
        <FileTable
          files={files}
          loading={loading}
          selectedFiles={selectedFiles}
          onSelect={onSelect}
          onNavigate={onNavigate}
          onDelete={mode === "remote" ? handleDelete : undefined}
          onRename={mode === "remote" ? handleRename : undefined}
          onCreateDir={
            mode === "remote" ? () => setMkdirVisible(true) : undefined
          }
        />
      )}

      <Modal
        title="新建文件夹"
        open={mkdirVisible}
        onOk={handleCreateDir}
        onCancel={() => {
          setMkdirVisible(false);
          setNewDirName("");
        }}
        okText="创建"
        cancelText="取消"
      >
        <Input
          placeholder="文件夹名称"
          value={newDirName}
          onChange={(e) => setNewDirName(e.target.value)}
          onPressEnter={handleCreateDir}
          autoFocus
        />
      </Modal>
    </Card>
  );
};

export default FilePanel;
