# ftx

基于 Tauri 2 + React 19 + TypeScript 构建的跨平台 FTP/SFTP 桌面客户端。

## 功能特性

### 主机管理
- 支持 FTP 与 SFTP 协议
- 主机配置持久化存储（名称、地址、端口、账号密码、SSH 密钥路径）
- 新增、编辑、删除主机
- 连接测试与一键连接/断开

### 文件浏览
- **双面板**：本地文件与远程文件并排展示
- **路径导航**：面包屑导航，支持进入目录、返回上级
- **远程操作**：新建目录、删除、重命名
- **目录上传/下载**：支持整目录递归传输

### 传输管理
- **传输队列**：多任务并发，实时进度、速度、剩余时间
- **传输历史**：所有传输记录持久化到 SQLite，支持按主机筛选
- **失败重试**：失败任务一键重试
- **取消传输**：进行中任务可取消

### 断点续传
- 大文件传输中断后可从上次进度继续
- 本地与远程文件大小校验

### 覆盖处理
- 目标文件已存在时的弹窗确认
- 支持：覆盖、跳过、重命名

## 技术栈

| 层级   | 技术                         |
|--------|------------------------------|
| 框架   | Tauri 2, React 19, Vite 7    |
| 前端   | TypeScript, Ant Design, Zustand |
| 后端   | Rust                         |
| 协议   | FTP (suppaftp), SFTP (ssh2)  |
| 存储   | SQLite (rusqlite)            |
| 测试   | Vitest, Testing Library      |

## 环境要求

- **Node.js** 18+
- **Rust** (stable)
- **pnpm / npm / yarn**

## 开发

```bash
# 安装依赖
npm install

# 启动开发模式（前端 + Tauri 窗口）
npm run tauri dev

# 仅运行前端
npm run dev

# 运行单元测试
npm run test
```

## 构建

```bash
# 构建生产包
npm run tauri build
```

构建产物位于 `src-tauri/target/release/bundle/`，支持 macOS、Windows、Linux。

## 项目结构

```
ftx/
├── src/                    # React 前端
│   ├── components/         # UI 组件
│   │   ├── FileBrowser/    # 双面板文件浏览
│   │   ├── HostManager/     # 主机管理
│   │   ├── TransferQueue/   # 传输队列
│   │   ├── TransferHistory/ # 传输历史
│   │   └── OverwriteDialog/ # 覆盖确认弹窗
│   ├── stores/             # Zustand 状态
│   ├── hooks/              # 自定义 Hooks
│   └── utils/              # 工具函数
├── src-tauri/              # Rust 后端
│   ├── src/
│   │   ├── commands/       # Tauri 命令（host/transfer/connection/bookmark 等）
│   │   ├── services/       # FTP 客户端、SFTP 客户端、传输引擎
│   │   ├── db/             # SQLite 模型与迁移
│   │   └── models/         # 数据模型
│   └── tauri.conf.json     # Tauri 配置
├── package.json
└── README.md
```

## IDE 推荐

- [VS Code](https://code.visualstudio.com/)
- [Tauri 扩展](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode)
- [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)

## License

MIT
