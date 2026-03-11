import React, { useCallback, useEffect } from "react";
import { Button, List, Typography, Popconfirm, message, Tooltip } from "antd";
import { DeleteOutlined } from "@ant-design/icons";
import { useBookmarkStore } from "../../stores/bookmarkStore";
import { useHostStore } from "../../stores/hostStore";
import { useFileBrowser } from "../../hooks/useFileBrowser";
import type { DirectoryBookmark } from "../../types";

const { Text } = Typography;

const BookmarkPanel: React.FC = () => {
  const { bookmarks, loading, fetchAll, deleteBookmark, touchBookmark } =
    useBookmarkStore();
  const hosts = useHostStore((s) => s.hosts);
  const { navigateToBookmark } = useFileBrowser();

  useEffect(() => {
    fetchAll();
  }, [fetchAll]);

  const handleClick = useCallback(
    async (bm: DirectoryBookmark) => {
      try {
        await navigateToBookmark(bm);
        await touchBookmark(bm.id!);
        message.success(`已跳转到 ${bm.label}`);
      } catch (err) {
        message.error(`导航失败: ${err}`);
      }
    },
    [navigateToBookmark, touchBookmark]
  );

  const handleDelete = useCallback(
    async (id: number, e?: React.MouseEvent) => {
      e?.stopPropagation();
      try {
        await deleteBookmark(id);
        message.success("收藏已删除");
      } catch (err) {
        message.error(`删除失败: ${err}`);
      }
    },
    [deleteBookmark]
  );

  const getHostName = (hostId: number) => {
    const host = hosts.find((h) => h.id === hostId);
    return host?.name ?? `主机 ${hostId}`;
  };

  return (
    <div
      style={{
        padding: 8,
        borderTop: "1px solid #f0f0f0",
        flexShrink: 0,
        height: "50vh",
        display: "flex",
        flexDirection: "column",
        minHeight: 0,
      }}
    >
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
          marginBottom: 8,
          flexShrink: 0,
        }}
      >
        <Text strong>收藏的目录</Text>
      </div>

      <div style={{ flex: 1, overflow: "auto", minHeight: 0 }}>
        <List
          loading={loading}
          dataSource={bookmarks}
          size="small"
          locale={{ emptyText: "暂无收藏，在远程目录中点击星标添加" }}
          renderItem={(bm) => (
            <List.Item
              style={{
                cursor: "pointer",
                padding: "4px 8px",
                borderRadius: 4,
              }}
              onClick={() => handleClick(bm)}
              actions={[
                <Popconfirm
                  key="del"
                  title="确认删除"
                  description={`确定要删除收藏「${bm.label}」吗？`}
                  onConfirm={(e) =>
                    handleDelete(bm.id!, e as React.MouseEvent | undefined)
                  }
                  onCancel={(e) => e?.stopPropagation()}
                  okText="删除"
                  cancelText="取消"
                  okButtonProps={{ danger: true }}
                >
                  <Tooltip title="删除收藏">
                    <Button
                      type="text"
                      size="small"
                      danger
                      icon={<DeleteOutlined />}
                      onClick={(e) => e.stopPropagation()}
                    />
                  </Tooltip>
                </Popconfirm>,
              ]}
            >
              <List.Item.Meta
                title={
                  <Text ellipsis style={{ maxWidth: 120 }}>
                    {bm.label}
                  </Text>
                }
                description={
                  <Text type="secondary" style={{ fontSize: 11 }}>
                    {getHostName(bm.host_id)} · {bm.remote_dir || "/"}
                  </Text>
                }
              />
            </List.Item>
          )}
        />
      </div>
    </div>
  );
};

export default BookmarkPanel;
