# Quota Float v0.2.4 发布前记录

更新时间：2026-07-22（Asia/Shanghai）

> 历史说明：付费皮肤授权与维护者签发工具已在后续版本移除；Default、Blur、Computer 现均为永久免费内置皮肤。本文件保留 v0.2.4 的通用发布记录与产物哈希。

## 当前状态

- 用户已确认 `v0.2.4` Windows/macOS 测试包可用。
- GitHub Release `v0.2.4` 保持为草稿，未经用户明确指示不得公开发布。
- 发布标签指向提交 `d3972fc64600a5bd2c7b6d3d2e071d36b7f37c5d`。
- Windows 与 macOS Universal GitHub Actions 均已成功。
- 草稿产物 SHA-256、版本信息以及解包后的隐私材料扫描均已通过。
- `Designer @Change` 是允许出现在用户界面的公开设计师标识。
- 用户已接受本版本不做 Windows Authenticode 签名和 Apple 公证；公开发布说明必须明确 SmartScreen/Gatekeeper 可能出现安全提示。

## 已完成的安全与隔离检查

- 用户包只包含根应用，不包含维护者工具、`.work-feedback` 或维护者本机路径。
- 用户包不包含私钥、测试材料、账本、买家/订单数据或临时文件。
- Release 工作流会校验标签版本，并使用独立的 Tauri 更新签名密钥。

## 正式公开发布前仍需完成

1. 在最终下载的 Windows 和 macOS 包上确认 Default、Blur、Computer 可自由切换且重启后保留。
2. 在真实 Mac 上确认 Gatekeeper 提示、启动、托盘、透明窗口和展开/收起。
3. 在 Windows 上确认 SmartScreen 提示、安装/卸载、托盘、自动启动以及 100%/125%/150% 缩放。
4. 人工校对草稿 Release 的更新说明，并加入 SmartScreen/Gatekeeper 安装提示。
5. 记录最终验收人、时间、Windows EXE/MSI 与 macOS DMG 的 SHA-256，再等待用户明确指示后手动公开发布。

## v0.2.4 草稿产物哈希

- `Quota.Float_0.2.4_x64-setup.exe`: `3308ECB2691216729097C982D19EAD818C65CBF0764179420BBF6B2237C9465D`
- `Quota.Float_0.2.4_x64_en-US.msi`: `B81A14ABDA043120BD1DF7793488082F1CE72204CDD78AACD4BE16BB0139B226`
- `Quota.Float_0.2.4_universal.dmg`: `8596DCE9FDA25CC88FB02A93C34589A966C6FA1C3D13AB3C4149DBBA5246F7B5`
- `Quota.Float_universal.app.tar.gz`: `0782DF96DE8CD24C17388A19190374C7B80128CFFB09DFDE21D4C1E1C526D4CD`
