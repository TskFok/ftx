import React, { useCallback, useEffect, useState } from "react";
import {
  Button,
  List,
  Typography,
  Popconfirm,
  message,
  Space,
  Tooltip,
} from "antd";
import {
  PlusOutlined,
  DeleteOutlined,
  EditOutlined,
  ApiOutlined,
  LoadingOutlined,
  LinkOutlined,
  DisconnectOutlined,
} from "@ant-design/icons";
import { useHostStore } from "../../stores/hostStore";
import { useFileBrowser } from "../../hooks/useFileBrowser";
import HostFormModal from "./HostFormModal";
import type { Host } from "../../types";

const { Text } = Typography;

const HostManager: React.FC = () => {
  const {
    hosts,
    currentHost,
    loading,
    fetchHosts,
    createHost,
    updateHost,
    deleteHost,
    setCurrentHost,
    testConnectionById,
  } = useHostStore();

  const { connectedHostId, connectAndBrowse, disconnectHost } =
    useFileBrowser();

  const [modalOpen, setModalOpen] = useState(false);
  const [editingHost, setEditingHost] = useState<Host | null>(null);
  const [saving, setSaving] = useState(false);
  const [testingId, setTestingId] = useState<number | null>(null);
  const [connectingId, setConnectingId] = useState<number | null>(null);

  useEffect(() => {
    fetchHosts();
  }, [fetchHosts]);

  const handleAdd = useCallback(() => {
    setEditingHost(null);
    setModalOpen(true);
  }, []);

  const handleEdit = useCallback((host: Host, e: React.MouseEvent) => {
    e.stopPropagation();
    setEditingHost(host);
    setModalOpen(true);
  }, []);

  const handleModalOk = useCallback(
    async (values: Host) => {
      setSaving(true);
      try {
        if (values.id) {
          await updateHost(values);
          message.success("主机更新成功");
        } else {
          await createHost(values);
          message.success("主机创建成功");
        }
        setModalOpen(false);
      } catch (err) {
        message.error(`操作失败: ${err}`);
      } finally {
        setSaving(false);
      }
    },
    [createHost, updateHost],
  );

  const handleDelete = useCallback(
    async (id: number, e?: React.MouseEvent) => {
      e?.stopPropagation();
      try {
        await deleteHost(id);
        message.success("主机已删除");
      } catch (err) {
        message.error(`删除失败: ${err}`);
      }
    },
    [deleteHost],
  );

  const handleTestConnection = useCallback(
    async (host: Host, e: React.MouseEvent) => {
      e.stopPropagation();
      if (!host.id) return;
      setTestingId(host.id);
      try {
        await testConnectionById(host.id);
        message.success(`连接 ${host.name} 成功`);
      } catch (err) {
        message.error(`连接失败: ${err}`);
      } finally {
        setTestingId(null);
      }
    },
    [testConnectionById],
  );

  const handleConnect = useCallback(
    async (host: Host, e: React.MouseEvent) => {
      e.stopPropagation();
      if (!host.id) return;

      if (connectedHostId === host.id) {
        try {
          await disconnectHost();
          message.info(`已断开 ${host.name}`);
        } catch (err) {
          message.error(`断开失败: ${err}`);
        }
        return;
      }

      setConnectingId(host.id);
      try {
        if (connectedHostId) {
          await disconnectHost();
        }
        await connectAndBrowse(host.id);
        setCurrentHost(host);
        message.success(`已连接 ${host.name}`);
      } catch (err) {
        message.error(`连接失败: ${err}`);
      } finally {
        setConnectingId(null);
      }
    },
    [connectedHostId, connectAndBrowse, disconnectHost, setCurrentHost],
  );

  return (
    <div
      style={{
        padding: 8,
        flex: 1,
        minHeight: 0,
        display: "flex",
        flexDirection: "column",
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
        <Text strong>主机列表</Text>
        <Button
          type="primary"
          size="small"
          icon={<PlusOutlined />}
          onClick={handleAdd}
        >
          新增
        </Button>
      </div>

      <div style={{ flex: 1, overflow: "auto", minHeight: 0 }}>
        <List
          loading={loading}
          dataSource={hosts}
          size="small"
          locale={{ emptyText: '暂无主机，点击上方"新增"添加' }}
          renderItem={(host) => (
            <List.Item
              style={{
                cursor: "pointer",
                background:
                  connectedHostId === host.id
                    ? "#e6f4ff"
                    : currentHost?.id === host.id
                      ? "#f5f5f5"
                      : undefined,
                padding: "6px 8px",
                borderRadius: 4,
              }}
              onClick={() => setCurrentHost(host)}
              actions={[
                <Space key="actions" size={0}>
                  <Tooltip
                    title={
                      connectedHostId === host.id ? "断开连接" : "连接"
                    }
                  >
                    <Button
                      type="text"
                      size="small"
                      icon={
                        connectingId === host.id ? (
                          <LoadingOutlined />
                        ) : connectedHostId === host.id ? (
                          <DisconnectOutlined style={{ color: "#52c41a" }} />
                        ) : (
                          <LinkOutlined />
                        )
                      }
                      disabled={connectingId === host.id}
                      onClick={(e) => handleConnect(host, e)}
                    />
                  </Tooltip>
                  <Tooltip title="测试连接">
                    <Button
                      type="text"
                      size="small"
                      icon={
                        testingId === host.id ? (
                          <LoadingOutlined />
                        ) : (
                          <ApiOutlined />
                        )
                      }
                      disabled={testingId === host.id}
                      onClick={(e) => handleTestConnection(host, e)}
                    />
                  </Tooltip>
                  <Tooltip title="编辑">
                    <Button
                      type="text"
                      size="small"
                      icon={<EditOutlined />}
                      onClick={(e) => handleEdit(host, e)}
                    />
                  </Tooltip>
                  <Popconfirm
                    title="确认删除"
                    description={`确定要删除主机「${host.name}」吗？`}
                    onConfirm={(e) =>
                      handleDelete(host.id!, e as React.MouseEvent | undefined)
                    }
                    onCancel={(e) => e?.stopPropagation()}
                    okText="删除"
                    cancelText="取消"
                    okButtonProps={{ danger: true }}
                  >
                    <Tooltip title="删除">
                      <Button
                        type="text"
                        danger
                        size="small"
                        icon={<DeleteOutlined />}
                        onClick={(e) => e.stopPropagation()}
                      />
                    </Tooltip>
                  </Popconfirm>
                </Space>,
              ]}
            >
              <List.Item.Meta
                title={
                  <Text ellipsis style={{ maxWidth: 100 }}>
                    {host.name}
                  </Text>
                }
                description={
                  <Text type="secondary" style={{ fontSize: 12 }}>
                    {host.protocol.toUpperCase()}://{host.host}:{host.port}
                  </Text>
                }
              />
            </List.Item>
          )}
        />
      </div>

      <HostFormModal
        open={modalOpen}
        host={editingHost}
        confirmLoading={saving}
        onOk={handleModalOk}
        onCancel={() => setModalOpen(false)}
      />
    </div>
  );
};

export default HostManager;
