import React, { useState, useEffect } from "react";
import { Layout as AntLayout, Button, Modal, Form, InputNumber, message } from "antd";
import { SettingOutlined } from "@ant-design/icons";
import { useSettingsStore } from "../../stores/settingsStore";

const { Header, Content, Sider } = AntLayout;

interface AppLayoutProps {
  sidebar: React.ReactNode;
  children: React.ReactNode;
}

const AppLayout: React.FC<AppLayoutProps> = ({ sidebar, children }) => {
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [form] = Form.useForm();
  const { idleTimeoutSecs, fetchIdleTimeout, setIdleTimeout } = useSettingsStore();

  useEffect(() => {
    fetchIdleTimeout();
  }, [fetchIdleTimeout]);

  useEffect(() => {
    if (settingsOpen) {
      form.setFieldsValue({ idleTimeoutSecs });
    }
  }, [settingsOpen, idleTimeoutSecs, form]);

  const handleSettingsOk = async () => {
    try {
      const values = await form.validateFields();
      await setIdleTimeout(values.idleTimeoutSecs);
      message.success("设置已保存");
      setSettingsOpen(false);
    } catch (e) {
      if (typeof e === "object" && e !== null && "errorFields" in e) {
        // 表单校验错误，不处理
      } else {
        message.error(String(e));
      }
    }
  };

  return (
    <AntLayout style={{ height: "100vh" }}>
      <Sider
        width={240}
        theme="light"
        style={{
          borderRight: "1px solid #f0f0f0",
          display: "flex",
          flexDirection: "column",
          overflow: "hidden",
        }}
      >
        <div
          style={{
            flex: 1,
            overflow: "hidden",
            display: "flex",
            flexDirection: "column",
            minHeight: 0,
          }}
        >
          {sidebar}
        </div>
      </Sider>
      <AntLayout>
        <Header
          style={{
            background: "#fff",
            padding: "0 16px",
            borderBottom: "1px solid #f0f0f0",
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            height: 48,
          }}
        >
          <h3 style={{ margin: 0 }}>ftx - FTP/SFTP 文件传输工具</h3>
          <Button
            type="text"
            icon={<SettingOutlined />}
            onClick={() => setSettingsOpen(true)}
            title="设置"
          />
        </Header>
        <Modal
          title="设置"
          open={settingsOpen}
          onOk={handleSettingsOk}
          onCancel={() => setSettingsOpen(false)}
          destroyOnClose
        >
          <Form form={form} layout="vertical">
            <Form.Item
              name="idleTimeoutSecs"
              label="空闲断开时间（秒）"
              rules={[
                { required: true, message: "请输入空闲断开时间" },
                {
                  type: "number",
                  min: 0,
                  max: 86400,
                  message: "0 表示不自动断开，最大 86400 秒（24 小时）",
                },
              ]}
              extra="连接空闲超过此时间将自动断开。设为 0 表示不自动断开。"
            >
              <InputNumber
                min={0}
                max={86400}
                style={{ width: "100%" }}
                placeholder="默认 300 秒（5 分钟）"
              />
            </Form.Item>
          </Form>
        </Modal>
        <Content style={{ padding: 16, overflow: "auto" }}>{children}</Content>
      </AntLayout>
    </AntLayout>
  );
};

export default AppLayout;
