import React from "react";
import { Layout as AntLayout } from "antd";

const { Header, Content, Sider } = AntLayout;

interface AppLayoutProps {
  sidebar: React.ReactNode;
  children: React.ReactNode;
}

const AppLayout: React.FC<AppLayoutProps> = ({ sidebar, children }) => {
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
            height: 48,
          }}
        >
          <h3 style={{ margin: 0 }}>ftx - FTP/SFTP 文件传输工具</h3>
        </Header>
        <Content style={{ padding: 16, overflow: "auto" }}>{children}</Content>
      </AntLayout>
    </AntLayout>
  );
};

export default AppLayout;
