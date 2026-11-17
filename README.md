# samba-ui

一个用 Rust 写的、**零第三方依赖**的 Samba `smb.conf` 可视化管理页面。单个二进制、内嵌前端资源，可直接跑在 ARMv7（树莓派、ARM NAS、老路由器）上。

- 只依赖 Rust 标准库：没有 Web 框架、没有模板引擎、没有数据库、没有前端构建链。
- 修改时保留注释、空行、参数顺序、缩进、大小写、CRLF/BOM。
- 写盘前校验（`testparm`）、写盘时原子替换、写盘后自动备份并可自动重载 Samba。
- 默认只监听 `127.0.0.1`，内置 HTTP Basic 认证、CSRF 防护、Origin 校验和严格 CSP。
- 提供中文界面：概览、全局设置、共享编辑、新建/删除共享、原始文件编辑。

> 页面使用明文 HTTP。远程使用时请通过 SSH 隧道或带 TLS 的反向代理访问，不要直接把端口暴露到公网。

## 快速开始

### 在本机试用

源码运行，只读预览示例配置，不会碰系统里的 Samba：

```sh
cargo run -- --conf examples/smb.conf --read-only --no-testparm --no-reload
```

Windows PowerShell：

```powershell
cargo run -- --conf examples\smb.conf --read-only --no-testparm --no-reload
```

然后打开 <http://127.0.0.1:8080/>。

### 直接在本机管理 Samba

```sh
cargo build --release
sudo ./target/release/samba-ui --conf /etc/samba/smb.conf
```

默认监听 `127.0.0.1:8080`。修改 `/etc/samba/smb.conf` 通常需要 root；如果配置在别的位置，用 `--conf /path/to/smb.conf`。

### 部署到 ARMv7 设备

最短路径（发布包已构建好的情况）：

```sh
scp dist/samba-ui-0.1.2-armv7.zip root@nas:/tmp/
ssh root@nas
cd /tmp
unzip samba-ui-0.1.2-armv7.zip -d samba-ui
cd samba-ui
sh deploy/install.sh ./samba-ui-armv7-musl
vi /etc/samba/samba-ui.env      # 设置 SAMBA_UI_USER / SAMBA_UI_PASSWORD
systemctl enable --now samba-ui
```

从本地通过 SSH 隧道访问：

```sh
ssh -L 8080:127.0.0.1:8080 root@nas
# 浏览器打开 http://127.0.0.1:8080/
```

完整步骤见[部署到 ARMv7](#部署到-armv7)，页面操作见[页面使用](#页面使用)。

## 命令行

| 参数 | 说明 |
| --- | --- |
| `--bind <IP>` | 监听地址，默认 `127.0.0.1`；只接受 IP 或 `localhost` |
| `--port <PORT>` | 监听端口，默认 `8080` |
| `--conf <PATH>` | `smb.conf` 路径，默认 `/etc/samba/smb.conf` |
| `--user <NAME>` | 启用 HTTP Basic 认证的用户名 |
| `--password <PASS>` | Basic 认证口令，建议改用环境变量 |
| `--reload <CMD>` | 保存成功后的重载命令，默认 `smbcontrol all reload-config`；空字符串表示禁用 |
| `--no-reload` | 禁用自动重载 |
| `--testparm <PATH>` | 语法校验程序，默认在 `PATH` 中查找 `testparm`；空字符串表示禁用 |
| `--no-testparm` | 禁用语法校验（不推荐） |
| `--backup-dir <DIR>` | 备份目录，默认 `<smb.conf 所在目录>/samba-ui-backups` |
| `--backups <N>` | 保留的备份数量，默认 `10`；`0` 表示不备份 |
| `--read-only` | 只读模式，页面可查看但拒绝所有保存 |
| `--allow-insecure` | 允许无认证监听非回环地址（不推荐） |
| `--allow-origin <ORIGIN>` | 额外允许的来源地址，如 `http://nas:8080`；可重复或用逗号分隔，用于反向代理改写 Host 的场景 |
| `--no-auth` | 强制关闭认证，仅用于本地调试 |
| `-h`, `--help` | 显示帮助 |
| `-V`, `--version` | 显示版本 |

对应环境变量：`SAMBA_UI_BIND`、`SAMBA_UI_PORT`、`SAMBA_UI_CONF`、`SAMBA_UI_USER`、`SAMBA_UI_PASSWORD`、`SAMBA_UI_RELOAD`、`SAMBA_UI_TESTPARM`、`SAMBA_UI_BACKUP_DIR`、`SAMBA_UI_BACKUPS`、`SAMBA_UI_ALLOW_ORIGINS`。命令行优先于环境变量。

### 典型用法

```sh
# 本地只读预览
./samba-ui --conf ./examples/smb.conf --read-only --no-testparm --no-reload

# 局域网访问，开启认证
sudo SAMBA_UI_PASSWORD='s3cret' ./samba-ui --bind 0.0.0.0 --user admin

# 自定义重载命令
sudo ./samba-ui --reload 'systemctl reload smbd'

# 关闭语法校验和自动重载（例如容器里没有 testparm/smbcontrol）
sudo ./samba-ui --no-testparm --no-reload

# 换端口
sudo ./samba-ui --port 8081

# 反向代理改写了 Host 时，显式放行浏览器的访问地址
sudo ./samba-ui --allow-origin http://nas:8080
```

## 部署到 ARMv7

### 选择二进制

构建脚本会在 `dist/` 生成两个产物：

| 文件 | 说明 | 运行要求 |
| --- | --- | --- |
| `dist/samba-ui-armv7-musl` | 静态链接，推荐 | 任意 ARMv7 硬浮点 Linux |
| `dist/samba-ui-armv7-glibc` | 动态链接 | glibc ≥ 2.34，且需要 `libgcc_s.so.1` |

两者都是 EABI5、hard-float、ELF32、ARM 架构。不确定设备 glibc 版本时，直接用 musl 版本。

### 方式 A：发布包一键安装（推荐）

把发布包传到设备并解压：

```sh
scp dist/samba-ui-0.1.2-armv7.zip root@nas:/tmp/
ssh root@nas
cd /tmp
unzip samba-ui-0.1.2-armv7.zip -d samba-ui
cd samba-ui
sh deploy/install.sh ./samba-ui-armv7-musl
```

[deploy/install.sh](deploy/install.sh) 会：

- 安装二进制到 `/usr/local/bin/samba-ui`
- 安装 systemd unit 到 `/etc/systemd/system/samba-ui.service`
- 首次创建 `/etc/samba/samba-ui.env`（权限 600）
- 安装 README 到 `/usr/local/share/doc/samba-ui/`
- 执行 `systemctl daemon-reload`，但不会自动启动服务

编辑认证配置：

```sh
vi /etc/samba/samba-ui.env
```

至少设置：

```ini
SAMBA_UI_USER=admin
SAMBA_UI_PASSWORD=换成一个强口令
```

然后启动：

```sh
systemctl enable --now samba-ui
systemctl status samba-ui
journalctl -u samba-ui -f
```

如果设备没有 `unzip`，可以先在本地解压，再用 `scp` 分别传二进制和 `deploy/` 目录，按方式 B 安装。

### 方式 B：手动安装

```sh
install -m 0755 samba-ui-armv7-musl /usr/local/bin/samba-ui
install -m 0644 deploy/samba-ui.service /etc/systemd/system/samba-ui.service
install -d -m 0750 /etc/samba
install -m 0600 deploy/samba-ui.env.example /etc/samba/samba-ui.env
vi /etc/samba/samba-ui.env
systemctl daemon-reload
systemctl enable --now samba-ui
```

unit 内容见 [deploy/samba-ui.service](deploy/samba-ui.service)。它以 root 运行，通过 `ProtectSystem=strict` + `ReadWritePaths=/etc/samba` 只允许写 Samba 配置目录。如果你把 `--backup-dir` 改到 `/etc/samba` 之外，需要同时把该目录加入 `ReadWritePaths`，否则备份会失败。

注意：unit 的 `ExecStart` 写死了 `--bind 127.0.0.1`，命令行会覆盖 `SAMBA_UI_BIND` 环境变量。要监听局域网，需要修改 unit 里的 `--bind`（见下文）。

### 方式 C：临时运行，不装服务

```sh
chmod +x samba-ui-armv7-musl
sudo ./samba-ui-armv7-musl --conf /etc/samba/smb.conf --port 8080
```

看到类似日志就成功了：

```text
samba-ui v0.1.2 已启动: http://127.0.0.1:8080/
配置文件: /etc/samba/smb.conf
语法校验: testparm；保存后重载: smbcontrol all reload-config
```

按 `Ctrl+C` 停止。

### 访问方式

#### SSH 隧道（推荐）

服务默认只监听回环地址，不额外暴露端口：

```sh
ssh -L 8080:127.0.0.1:8080 root@nas
```

本地浏览器打开：

```text
http://127.0.0.1:8080/
```

输入 `SAMBA_UI_USER` / `SAMBA_UI_PASSWORD` 对应的账号密码。

#### 局域网直连

直接运行时：

```sh
sudo SAMBA_UI_PASSWORD='强口令' ./samba-ui-armv7-musl \
  --conf /etc/samba/smb.conf \
  --bind 0.0.0.0 \
  --port 8080 \
  --user admin
```

使用 systemd 时，把 unit 的 `ExecStart` 改成：

```ini
ExecStart=/usr/local/bin/samba-ui --conf /etc/samba/smb.conf --bind 0.0.0.0 --port 8080
```

然后：

```sh
systemctl daemon-reload
systemctl restart samba-ui
```

并确保 `/etc/samba/samba-ui.env` 中已设置用户名和口令。如果设备有防火墙，还需要放行对应端口。

> 没有认证时监听非回环地址会直接启动失败；除非显式加 `--allow-insecure`，但不建议这样做。

#### 反向代理 + TLS

可以在前面放 Nginx/Caddy 做 TLS 终止。Nginx 示例：

```nginx
location / {
    proxy_pass http://127.0.0.1:8080;
    proxy_set_header Host $http_host;
    proxy_set_header X-Forwarded-Host $http_host;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_set_header X-Forwarded-Proto $scheme;
}
```

必须保留浏览器地址中的 Host 和端口。Nginx 的 `$host` 会丢掉端口，这是保存时报 403 最常见的原因；应使用 `$http_host`。Caddy/Traefik 通常会自动发送 `X-Forwarded-Host`，无需额外配置。

如果代理无法保留原始 Host，启动时显式放行浏览器的访问地址：

```sh
samba-ui --allow-origin http://nas:8080
# 多个地址用逗号分隔，或重复 --allow-origin
samba-ui --allow-origin http://nas:8080,https://nas.example.com
```

服务本身不支持 TLS，不要用 `--allow-insecure` 来绕过认证。

### 从源码交叉编译

#### Windows 本机

需要先安装 Rust 目标和本地 ARM GNU Toolchain（默认路径见脚本）：

```powershell
rustup target add armv7-unknown-linux-musleabihf armv7-unknown-linux-gnueabihf
.\scripts\build-armv7.ps1
```

如果工具链在别的目录：

```powershell
.\scripts\build-armv7.ps1 -Toolchain 'D:\arm-gnu-toolchain\...\arm-none-linux-gnueabihf'
```

脚本会构建两个目标，并把产物复制到 `dist/`，同时打印 SHA256。

#### Linux 主机

```sh
rustup target add armv7-unknown-linux-musleabihf armv7-unknown-linux-gnueabihf
ARM_GCC=arm-linux-gnueabihf-gcc ./scripts/build-armv7.sh
```

脚本在 `dist/` 生成二进制和 `SHA256SUMS`。

### 校验产物

```sh
file /usr/local/bin/samba-ui
# ELF 32-bit LSB executable, ARM, EABI5 version 1 (SYSV), statically linked, ...
```

也可以校验发布包：

```sh
sha256sum -c SHA256SUMS
```

## 页面使用

### 概览

显示配置文件路径、大小、语法校验/重载状态、认证状态、备份目录和共享列表。

页面顶部的黄色提示通常是：

- 未启用认证
- 没找到 `testparm` 或 `smbcontrol`
- 配置文件带只读权限标记
- 存在多个 `[global]` 段
- 第一个配置段之前存在参数行

### 全局设置

- “常用参数”是表单：工作组、`security`、协议版本、日志、字符集等。
- **留空表示删除该参数**。
- 下拉框会保留无法识别的当前值，不会因为打开页面再保存就改写。
- “其他参数”以表格编辑未知参数，可逐行删除。
- “新增参数”可以追加任意合法参数。
- 点“保存全局设置”才会写盘。

### 共享

- 概览里点共享名或“编辑”进入共享页。
- 可编辑 `path`、`comment`、`read only`、`guest ok`、`valid users`、权限掩码等。
- 修改共享名、调整段顺序请用“原始编辑”。
- 页面底部有“删除此共享”，会二次确认。

### 新建与删除共享

新建共享：

- 共享名不能是 `global` 或 `new`，不能含 `[ ] / \`。
- `path` 必填，且必须是服务器上真实存在的目录（服务只检查非空，不检查目录是否存在）。
- 可同时添加任意额外参数。

删除共享：

- 删除整个配置段，保留段间空行分隔。
- 删除前会弹出确认框，确认后立即写入并重载。

### 原始编辑

直接编辑整个 `smb.conf`。保存时会：

1. 检查文件是否被其他人改过（冲突返回 409）
2. `testparm` 语法校验
3. 备份原文件
4. 原子替换
5. 自动重载 Samba

适合改共享名、整理段顺序、批量替换等高级操作。

### 保存之后

- 成功：303 跳转并提示“已保存”。
- 校验失败：页面显示 `testparm` 输出和未保存的草稿，**原文件不变**。
- 重载失败：文件已写入并备份，但显示重载命令输出和备份路径。
- 冲突：提示“配置已被外部修改”，刷新后重做。
- 每次成功保存默认生成一份备份，保留最近 10 份（可配置）。

重载失败时可以手动执行：

```sh
smbcontrol all reload-config
```

需要回滚时，从备份目录复制回原文件再重载：

```sh
ls /etc/samba/samba-ui-backups/
cp /etc/samba/samba-ui-backups/smb.conf.<时间戳>-000.bak /etc/samba/smb.conf
smbcontrol all reload-config
```

## 安全模型

- **默认回环地址**：`--bind` 默认 `127.0.0.1`；非回环地址且无认证时启动直接失败，除非显式 `--allow-insecure`。
- **Basic 认证**：设置 `--user`/`--password` 后所有页面（含静态资源）都要求认证；认证失败会延迟 300ms。
- **CSRF**：每个表单带进程级随机 `_csrf` 隐藏字段，POST 必须匹配。
- **Origin/Referer 校验**：POST 若带 `Origin`/`Referer`，其主机必须与 `Host` 或 `X-Forwarded-Host` 一致（默认端口 80/443 视为等价）。`Origin: null` 不再直接拒绝，而是回退到 CSRF token 校验（部分浏览器、隐私插件、沙箱或内嵌预览会这样发送）。代理改写 Host 时可用 `--allow-origin` 显式放行。
- **CSP**：`default-src 'none'; style-src 'self'; script-src 'self'; ...`，前端没有内联脚本/样式。
- **HTML 转义**：共享名、参数值、错误输出均转义，避免配置内容触发 XSS。
- **原子写入**：临时文件 + `fsync` + `rename`；写入前保留时间戳备份；不会出现写一半的配置。
- **并发冲突**：保存时比较 revision（mtime+长度），期间被其他程序改过则返回 409，不覆盖别人的修改。
- **只读模式**：`--read-only` 在路由层拒绝所有写请求。
- **进程加固**：systemd unit 启用 `ProtectSystem=strict`、`ProtectHome`、`PrivateTmp` 等选项。

没有 TLS。远程访问请使用 SSH 隧道，或在前面放 Nginx/Caddy 做 TLS 终止。

## 编辑行为

- 页面只编辑 `[global]` 和各个共享段；未知参数、注释、空行原样保留。
- 常用参数用表单编辑；其他参数以“参数名 / 值”表格编辑，可逐行删除或新增。
- 常用字段留空表示删除该参数；下拉框会保留无法识别的当前值，避免误改。
- 新参数追加到段末，沿用文件中已有的缩进风格。
- 删除共享会删除整个段，但保留段间空行分隔。
- “原始编辑”直接修改整个文件，保存前同样做校验、备份和原子替换。
- 修改共享名、调整段顺序等高级操作请使用原始编辑。

保存流程：

1. 读取并确认 revision 未变化；
2. 生成新文本（只改动发生变化的行）；
3. 写入同目录临时文件并 `fsync`；
4. 执行 `testparm -s <临时文件>`（若可用），失败则删除临时文件、原文件不变；
5. 备份原文件并清理超出数量的旧备份；
6. 原子替换 `smb.conf`；
7. 执行重载命令（若配置了），失败会显示命令输出和备份路径。

## 构建与测试

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

测试包括 52 个库单元测试和 5 个端到端 HTTP 流程测试（真实监听随机端口、Basic 认证、CSRF、创建/编辑/删除共享、原始编辑、revision 冲突、只读模式、反向代理 Host、`allow-origin`、`Origin: null` 回退）。

`cargo test` 的临时目录优先使用 `SAMBA_UI_TEST_TMP`，否则用系统临时目录，再退回 `target/test-tmp`。

## 目录结构

```
src/
  main.rs      二进制入口
  lib.rs       模块导出与 run()
  args.rs      命令行/环境变量
  app.rs       服务装配、监听循环、连接限制
  routes.rs    路由、表单解析、CSRF/Origin、提交逻辑
  pages.rs     各页面 HTML
  html.rs      页面骨架与表单控件
  conf.rs      smb.conf AST、最小差分编辑
  store.rs     加载/校验/备份/原子写入/重载
  http.rs      手写 HTTP/1.1 解析与响应
  util.rs      日志、时间、编码、子进程、常量时间比较
assets/        内嵌的 CSS/JS
examples/      示例 smb.conf
deploy/        systemd unit、环境变量示例、安装脚本
scripts/       ARMv7 交叉编译脚本
tests/         端到端 HTTP 测试
```

## 发布包内容

`dist/samba-ui-0.1.2-armv7.zip` 解压后包含：

```
samba-ui-armv7-musl      静态链接二进制（推荐）
samba-ui-armv7-glibc     动态链接二进制
README.md
SHA256SUMS
deploy/install.sh
deploy/samba-ui.service
deploy/samba-ui.env.example
examples/smb.conf
```

## 限制

- 仅支持 UTF-8 编码的 `smb.conf`（非 UTF-8 会拒绝编辑）。
- 单次请求体上限 4 MiB、请求头上限 16 KiB、表单字段上限 4096 个；编辑超大文件请直接用编辑器。
- 不处理 Samba 的 `include` 文件；只编辑 `--conf` 指定的主文件。
- 没有 TLS、没有用户体系、没有审计日志（只有标准输出访问日志）。
- 不做信号处理；收到 SIGTERM 由 systemd 直接结束，原子写入保证文件不会写坏。
- 常用字段留空会删除参数；要写空值请使用“其他参数/新增参数”。
- 依赖 `testparm` 与 `smbcontrol` 才能做校验和自动重载；没有它们时请加 `--no-testparm --no-reload`。
- 删除共享、修改共享名等操作没有回收站，只有保存前的备份。

## 常见问题

| 现象 | 处理 |
| --- | --- |
| 浏览器弹登录框，输密码仍 401 | 检查 `/etc/samba/samba-ui.env` 的 `SAMBA_UI_USER/PASSWORD`，改完 `systemctl restart samba-ui` |
| POST 返回 403 | 页面过期导致 CSRF 失效，刷新重试；若提示 Origin/Host 不匹配，按错误页列出的地址修正反向代理，或配置 `--allow-origin`；显示 `Origin: null` 时升级到 0.1.2 |
| 返回 409 | 文件被其他程序改过，刷新页面后重新编辑 |
| “语法校验未通过” | 看页面里的 `testparm` 输出，修正后重存；原文件没有被改 |
| “重载失败” | 文件已保存；检查 `smbcontrol` 是否在 PATH、smbd 是否运行，或手动 `smbcontrol all reload-config` |
| 启动报“无法打开配置文件” | 路径不对或权限不足；修改 `/etc/samba/smb.conf` 通常要 root |
| 端口被占用 | 加 `--port 8081` |
| 没有 `testparm` | Debian/Ubuntu 安装 `samba-common-bin`，或临时 `--no-testparm` |
| 设备上运行报 GLIBC 版本错误 | 换用静态 musl 版本 |
| 忘记密码 | 修改 `/etc/samba/samba-ui.env` 后 `systemctl restart samba-ui` |

**启动报“无法打开配置文件”**

确认路径存在，并且当前用户有读权限；修改 `/etc/samba/smb.conf` 通常需要 root。

**页面提示“语法校验未通过”**

页面会显示 `testparm` 输出，原文件没有被修改；修正后再次保存即可。也可以临时加 `--no-testparm` 排查，但不建议长期关闭。

**保存后提示“重载失败”**

文件已经写入并备份，但重载命令执行失败。检查 `smbcontrol` 是否在 `PATH`、Samba 是否运行，或手动执行：

```sh
smbcontrol all reload-config
```

**反向代理后 POST 返回 403**

403 页面会列出服务器看到的 `Host`、`Origin`/`Referer` 和 `X-Forwarded-Host`，对照浏览器地址即可定位差异。常见处理：

1. Nginx 保留浏览器地址中的主机和端口：

   ```nginx
   proxy_set_header Host $http_host;
   proxy_set_header X-Forwarded-Host $http_host;
   ```

   `$host` 会丢掉端口，是这类 403 最常见的原因。

2. Caddy/Traefik 通常会自动发送 `X-Forwarded-Host`，升级到 0.1.2 后无需额外配置。

3. 代理无法保留 Host 时，显式放行浏览器地址：

   ```sh
   samba-ui --allow-origin http://nas:8080
   ```

   systemd 部署可写进 `/etc/samba/samba-ui.env`：

   ```ini
   SAMBA_UI_ALLOW_ORIGINS=http://nas:8080
   ```

   多个地址用逗号分隔，改完执行 `systemctl restart samba-ui`。

**保存时错误页显示 `Origin: null`**

这是浏览器发送的 `Origin` 头，不是服务器配置问题。隐私插件、沙箱 iframe、部分内嵌预览或浏览器隐私模式会把它设为 `null`。0.1.2 起不再直接拒绝，而是回退到 CSRF token 校验；升级后即可正常保存。CSRF token 仍然必须正确，因此不会降低跨站防护。

**忘记密码**

密码只来自启动参数/环境变量，直接修改 `/etc/samba/samba-ui.env` 后 `systemctl restart samba-ui`。
