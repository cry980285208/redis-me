# RedisME 上架 Linux：Flathub + Snap 双轨操作手册

> **实现状态**：待实施  
> **App ID（已定）**：`com.hepengju.redis`（与 `tauri.conf.json` / 官网 [hepengju.com](https://www.hepengju.com) 一致）  
> **Snap 名（建议）**：`redis-me`（注册前先查是否被占；注册后不可改）  
> **说明**：进度只改本文；不要回写 `docs/zh/changelog/future.md`

对标 Redis Insight：Microsoft Store、Mac App Store、**Snapcraft**、**Flathub**、Docker Hub。我们 Linux 做 **Flathub + Snap**，GitHub Release 的 deb/rpm/AppImage 继续保留。

---

## 你需要准备什么

| 项                                                                 | 用途                                               |
| ------------------------------------------------------------------ | -------------------------------------------------- |
| Linux 机（建议 Ubuntu 22.04/24.04）                                | 本地打 Snap / Flatpak、试装                        |
| GitHub 账号（已开 **2FA**）                                        | Flathub 提交与后续写权限                           |
| Ubuntu One 账号                                                    | Snap Store 发布者                                  |
| 官网 `https://www.hepengju.com`（或 `hepengju.com`）可部署静态文件 | Flathub **已验证开发者**（上架后做）               |
| 已发布的 GitHub Release（含 AppImage）                             | 首版 Flatpak/Snap 包装现成产物，少踩 WebKit/构建坑 |

**建议顺序**：应用侧检测 → Snap 注册与上架 → Flatpak 本地包 → Flathub 提交 → 官网徽章 →（可选）Flathub 域名验证。

---

## 第 0 步：应用侧改造（两条轨道共用）✅

目的：Flatpak / Snap 安装时走「商店版」逻辑，**关掉 Tauri 内置更新**（与微软商店一致）。

### 0.1 `src-tauri/src/utils/app_store.rs`（已实现）

| 环境    | 判定                                         |
| ------- | -------------------------------------------- |
| Flatpak | `FLATPAK_ID` 已设置，或存在 `/.flatpak-info` |
| Snap    | `SNAP` 已设置                                |

Windows / macOS 逻辑不变。前端 `Setting.vue` / `AppMain.vue` 已按 `isAppStore` 分支，无需再改。

### 0.2 自测清单

- [ ] 官网直装：仍可检查更新
- [ ] `FLATPAK_ID=com.hepengju.redis` 启动（或设 `SNAP=/snap/redis-me/current`）：设置页为商店更新、不弹自更新

---

## 第 1 步：Snap Store——注册与上架

官方：[注册 snap](https://documentation.ubuntu.com/snapcraft/latest/how-to/publishing/register-a-snap/) · [发布修订](https://documentation.ubuntu.com/snapcraft/latest/how-to/publishing/manage-revisions-and-releases/)

### 1.1 注册发布者账号（网页）

1. 打开 https://login.ubuntu.com → 注册 / 登录 **Ubuntu One**（建议开 2FA）。
2. 打开 https://snapcraft.io/account → 用同一账号登录。
3. 填写 **Publisher display name**（商店里显示的作者名，如 `hepengju` / `RedisME`）。
4. 可选：上传头像、填官网 `https://www.hepengju.com`。

### 1.2 本机安装工具并登录

在 Ubuntu 上：

```bash
sudo snap install snapcraft --classic
# 若尚未安装 snapd：sudo apt install snapd

snapcraft login
# 按提示输入 Ubuntu One 邮箱、密码、2FA
snapcraft whoami   # 确认登录成功
```

CI 以后可用导出凭据（勿提交仓库）：

```bash
snapcraft export-login snapcraft-creds.txt
# Actions 里：SNAPCRAFT_STORE_CREDENTIALS=$(cat snapcraft-creds.txt)
```

### 1.3 注册包名 `redis-me`

包名全局唯一、**注册后不能改**。先试：

```bash
snapcraft register redis-me
```

- 成功：继续。
- 已被占用：试 `redisme`、`redisme-gui` 等，或网页 https://snapcraft.io/account → **Register a snap name**。
- `snapcraft.yaml` 里的 `name:` **必须**与注册名完全一致。

商店里给人看的标题用 `title: RedisME`（可与 `name` 不同）。

### 1.4 `snap/snapcraft.yaml`（对齐 Tauri 官方：包装 deb）

对照：[Tauri Snap 文档](https://v2.tauri.app/distribute/snapcraft/)、[faire-todo-app](https://github.com/charlesschaefer/faire-todo-app)、addiction-tracker——均为 `tauri build --bundles deb` → `dpkg -x`。**不用 AppImage**（自带 GTK 易与 portal / 文件对话框冲突）。

| 项    | 约定                                                                              |
| ----- | --------------------------------------------------------------------------------- |
| 版本  | `version` 与 `package.json` 一致（build 校验）                                    |
| 输入  | `src-tauri/target/release/bundle/deb/RedisME_<ver>_amd64.deb`；可 `REDIS_ME_DEB=` |
| 图标  | deb 内 hicolor + `Icon=/usr/share/icons/hicolor/...`                              |
| plugs | `gnome` + `network` / `home` / `removable-media` / `unity7`；`GTK_USE_PORTAL=1`   |
| 本地  | `snapcraft pack --destructive-mode`（**勿 sudo**；sudo 易搞坏 apt 缓存与 portal） |

### 1.5 本地构建与试装

```bash
cd ~/redis-me
vp run tauri build
ls src-tauri/target/release/bundle/deb/RedisME_*_amd64.deb

export https_proxy=http://192.168.1.111:7897
export http_proxy=http://192.168.1.111:7897
snapcraft pack --destructive-mode

sudo snap remove redis-me || true
sudo snap install ./redis-me_*.snap --dangerous
redis-me
# 测：图标、导出连接路径对话框、连 Redis、商店版无自更新
```

### 1.6 上传与渠道发布

首版可手动；配方稳定后改由 Actions 上传（secret：`SNAPCRAFT_STORE_CREDENTIALS`，见 1.2）。

```bash
# 先上 edge，自己再 snap install redis-me --edge 验证
snapcraft upload ./redis-me_*.snap --release=edge

# 无问题后晋升（revision 以 upload 输出为准）
snapcraft release redis-me <revision> stable
```

网页可在 https://snapcraft.io/redis-me/releases 看修订与渠道。

### 1.6.1 GitHub Actions 自动打 Snap（待接，目标形态）

在 `release.yml` 的 **ubuntu amd64** 矩阵任务末尾（`tauri-action` 已产出 **deb** 之后），或独立 `needs: publish-tauri` 的 job：

1. 安装 `snapcraft`
2. `snapcraft pack --destructive-mode`（读默认 Tauri **deb** 路径）
3. `snapcraft upload *.snap --release=edge`（凭据来自 secret）
4. 稳定后改为 `stable`，或保留手动晋升

Release 说明里可补一行：`sudo snap install redis-me`。

### 1.7 商店列表信息（网页）

登录 https://snapcraft.io/account → 选中 `redis-me`：

- [ ] 图标、截图（与官网一致即可）
- [ ] 简介、分类（如 Productivity / Utilities / Developer Tools）
- [ ] 许可证、源码链接 `https://github.com/hepengju/redis-me`
- [ ] 联系邮箱

### 1.8 Snap 验收

- [ ] https://snapcraft.io/redis-me 可打开
- [ ] `sudo snap install redis-me` 可装、可连 Redis
- [ ] `isAppStore` 为真
- [ ] 新版本可走 `upload` → `edge` → `stable`

---

## 第 2 步：Flathub——域名、打包、注册式提交

官方：[Requirements](https://docs.flathub.org/docs/for-app-authors/requirements) · [Submission](https://docs.flathub.org/docs/for-app-authors/submission) · [MetaInfo](https://docs.flathub.org/docs/for-app-authors/metainfo-guidelines) · [Verification](https://docs.flathub.org/docs/for-app-authors/verification)

竞品仓：[flathub/com.redis.RedisInsight](https://github.com/flathub/com.redis.RedisInsight)（用上游 AppImage）。

### 2.1 App ID 与域名（已定）

| 项                     | 值                                                    |
| ---------------------- | ----------------------------------------------------- |
| Flatpak / AppStream ID | **`com.hepengju.redis`**                              |
| 对应域名               | **`hepengju.com`**（reverse-DNS：`com` + `hepengju`） |
| 官网                   | https://www.hepengju.com                              |

提交审核时要求：你对域名有控制权，且相关 HTTPS 可访问。  
**蓝勾「Verified」**：应用已上架并拿到 flathub 应用仓写权限后，在 Developer Portal 做（见 2.8）；**不是**提交 PR 的前置条件，但建议尽早做。

验证文件最终形态（token 以上架后 Portal 显示为准）：

```text
https://hepengju.com/.well-known/org.flathub.VerifiedApps.txt
```

（若只部署在 `www`，需保证访问 `hepengju.com` 能 **HTTPS 跳转**到该文件；Flathub 跟随重定向，但必须 HTTPS。）

也可先在官网加一句「RedisME Flatpak ID: com.hepengju.redis」，方便审核看到关联。

### 2.2 本机 Flatpak 环境

```bash
sudo apt install flatpak flatpak-builder
flatpak remote-add --if-not-exists --user flathub https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak install -y flathub org.flatpak.Builder
# 再按 manifest 安装对应 runtime，例如：
# flatpak install -y flathub org.gnome.Platform//47 org.gnome.Sdk//47
```

### 2.3 在本仓库准备草稿（可选，方便本地迭代）

```text
flatpak/
  com.hepengju.redis.yml
  com.hepengju.redis.metainfo.xml
  # 图标可引用仓库 icons，或在 manifest 里从源拷贝
```

**首版策略**：包装 GitHub Release 的 **AppImage**（或 deb），manifest 写死 URL + sha256/sha512。  
Runtime：优先 `org.gnome.Platform`（Tauri/WebKit 常见）；版本用 Flathub 当时推荐（如 47），以审核意见为准。

`finish-args` 起步建议（再按审核收紧/放宽）：

```yaml
finish-args:
  - --share=ipc
  - --share=network
  - --socket=fallback-x11
  - --socket=wayland
  - --device=dri
  - --filesystem=home
  # SSH 隧道若必需再加，例如：
  # - --filesystem=xdg-run/gnupg:ro
  # - --filesystem=~/.ssh:ro
```

`metainfo.xml` 必须：

- `id` = `com.hepengju.redis`
- 名称 RedisME、summary、description（建议中英）
- `developer` / 项目主页 `https://www.hepengju.com`
- 许可证、分类（如 `Network`）、`content_rating`
- 截图（可指向官网或 GitHub raw，正式构建会 mirror）
- `<releases>` 含当前版本

校验：

```bash
flatpak run --command=flatpak-builder-lint org.flatpak.Builder appstream flatpak/com.hepengju.redis.metainfo.xml
```

### 2.4 本地构建、安装、跑通

```bash
cd flatpak   # 或仓库根，按你放 manifest 的路径
flatpak run --command=flathub-build org.flatpak.Builder --install com.hepengju.redis.yml

flatpak run com.hepengju.redis

flatpak run --command=flatpak-builder-lint org.flatpak.Builder manifest com.hepengju.redis.yml
flatpak run --command=flatpak-builder-lint org.flatpak.Builder repo repo
```

功能清单：连 Redis、文件导入导出、剪贴板、SSH（若声明了权限）。

### 2.5 向 Flathub「注册」新应用（GitHub PR，不是网页表单）

Flathub **没有**单独注册页；**第一次上架 = 对 `flathub/flathub` 的 `new-pr` 分支提 PR**。

#### A. Fork

1. 打开 https://github.com/flathub/flathub
2. **Fork**（取消勾选 “Copy the `master` branch only”，需要能看到 `new-pr`）

或用 CLI：

```bash
gh repo fork flathub/flathub --clone
cd flathub
git fetch origin
git checkout --track origin/new-pr
```

#### B. 建提交分支并放入文件

```bash
git checkout -b add-com.hepengju.redis new-pr

# 在仓库根放入（文件名与 ID 一致）：
#   com.hepengju.redis.yml
#   com.hepengju.redis.metainfo.xml
#   以及审核要求的其它文件（如 flathub.json）

git add com.hepengju.redis.yml com.hepengju.redis.metainfo.xml
git commit -m "Add com.hepengju.redis"
git push -u origin add-com.hepengju.redis
```

#### C. 开 PR

- **Base 仓库**：`flathub/flathub`
- **Base 分支**：必须是 **`new-pr`**（禁止对 `master`）
- **标题**：`Add com.hepengju.redis`
- 正文可写：简介、源码 URL、许可证、是否官方维护、测试说明

请先阅读并遵守 Flathub 的 Generative AI / Requirements，避免 PR 被直接关。

#### D. 审核互动

1. 回复 reviewer 评论（权限、元数据、沙箱等）。
2. **不要**为改意见而关 PR；继续 push 即可。
3. 评论触发试构建：`bot, build`（或文档当时写法 `bot, build com.hepengju.redis`）。
4. 按 bot 给的 flatpakref 试装。

#### E. 通过之后

1. 审核合并后，会创建 GitHub 仓 **`flathub/com.hepengju.redis`**。
2. 你的 GitHub 账号会收到 **Collaborator 邀请**（需已开 **2FA**），**一周内接受**。
3. 之后发版只改这个应用仓，不再走 `new-pr` 全流程。
4. 首次官方构建约 1～数小时后出现在 https://flathub.org/apps/com.hepengju.redis

维护文档：https://docs.flathub.org/docs/for-app-authors/maintenance

### 2.6 日常发新版本（Flathub 应用仓）

在 `flathub/com.hepengju.redis`：

1. 改 `com.hepengju.redis.yml`：版本、AppImage URL、sha256/sha512。
2. 改 `com.hepengju.redis.metainfo.xml`：在 `<releases>` **最前**插入新版本说明。
3. Push / PR（按该仓惯例）→ 等 Flathub CI 构建发布。

与 GitHub Release **同版本号**，避免用户困惑。

### 2.7 Flathub 验收

- [ ] https://flathub.org/apps/com.hepengju.redis
- [ ] `flatpak install flathub com.hepengju.redis`
- [ ] GNOME Software / KDE Discover 能搜到 RedisME
- [ ] 商店版无内置更新

### 2.8（上架后）域名验证拿蓝勾

1. 登录 https://flathub.org → **Developer Portal** → `com.hepengju.redis` → **Verification**。
2. 复制 token，放到官网：

```bash
# 部署到站点根，使下面 URL 可公开访问（HTTPS）
# https://hepengju.com/.well-known/org.flathub.VerifiedApps.txt
#
# 文件内容示例（token 以 Portal 为准）：
# # com.hepengju.redis
# xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx
```

3. 在 Portal 点验证；通过后应用页显示已验证开发者。
4. 验证文件建议长期保留。

---

## 第 3 步：官网与 Release 文案

| 位置                                        | 动作                               |
| ------------------------------------------- | ---------------------------------- |
| 官网 Linux 下载区                           | 增加 Snap / Flathub 徽章与安装命令 |
| `.github/workflows/release.yml` 的 Linux 行 | 增加商店链接（直装包仍保留）       |
| 本文文首                                    | 全部验收后把「实现状态」改为已完成 |

安装命令示例（上架后）：

```bash
# Snap
sudo snap install redis-me

# Flatpak
flatpak install flathub com.hepengju.redis
```

---

## 检查清单（按时间线勾）

### A. 账号与注册

- [ ] Ubuntu One + snapcraft.io/account 资料填好
- [ ] `snapcraft login` + `snapcraft register redis-me` 成功
- [ ] GitHub 2FA 已开
- [ ] 确认 `hepengju.com` HTTPS 可用，后续能放 `.well-known`

### B. 应用

- [x] Linux Flatpak/Snap 下 `is_app_store()` 为 true，updater 关闭（`app_store.rs` 已实现；真机/设环境变量自测）

### C. Snap

- [ ] `snap/snapcraft.yaml` 本地构建通过
- [ ] `--dangerous` 试装通过
- [ ] `edge` → `stable` 已发布
- [ ] 商店页截图/简介齐全

### D. Flathub

- [ ] 本地 `flathub-build` + lint 通过
- [ ] PR → `flathub/flathub` 的 **`new-pr`**，标题 `Add com.hepengju.redis`
- [ ] 审核通过，接受 `flathub/com.hepengju.redis` 邀请
- [ ] 商店页可安装
- [ ]（可选）域名验证蓝勾

### E. 对外

- [ ] 官网 + Release 双商店入口

---

## 风险速查

| 问题                           | 处理                                                         |
| ------------------------------ | ------------------------------------------------------------ |
| Snap 名被占                    | 换名并同步改 yaml / 文档                                     |
| strict 沙箱连不上 / 读不了密钥 | 逐步加 plugs；文档说明限制                                   |
| Flatpak 审核要求源码构建       | 再上 cargo/node generator；首版先声明校验和的 Release 二进制 |
| WebKit 缺库                    | 换 runtime 版本或补依赖；参考已上架 Tauri 应用 manifest      |
| 自更新与商店抢更新             | 务必完成第 0 步检测                                          |

---

## 参考链接

- Insight 桌面安装说明：https://redis.io/docs/latest/operate/redisinsight/install/install-on-desktop/
- Flathub 提交：https://docs.flathub.org/docs/for-app-authors/submission
- Flathub 验证：https://docs.flathub.org/docs/for-app-authors/verification
- Snap 注册：https://documentation.ubuntu.com/snapcraft/latest/how-to/publishing/register-a-snap/
- Snap 发布：https://documentation.ubuntu.com/snapcraft/latest/how-to/publishing/manage-revisions-and-releases/
- Insight Flathub 仓：https://github.com/flathub/com.redis.RedisInsight
- Tauri Flatpak/Snap 实战：https://vincent.jousse.org/blog/en/packaging-tauri-v2-flatpak-snapcraft-elm/

## 相关代码

- `src-tauri/tauri.conf.json` — `identifier`: `com.hepengju.redis`
- `src-tauri/src/utils/app_store.rs` — 商店检测
- `.github/workflows/release.yml` — Linux 产物与 Release 文案
- `src/views/ext/Setting.vue`、`src/views/AppMain.vue` — `isAppStore`
