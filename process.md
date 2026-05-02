# GVG NP Modding Process

## 目标
- 找到 `pl0al.pzz` 替换后“无限加载”的根因。
- 建立可重复、可验证、可回滚的实验流程。
- 在不破坏加载稳定性的前提下，把 `testout.dae` 成功落地到可见模型改动。

## 当前结论（已验证）
- `PMF2` 本体不是唯一问题来源：  
  仅改 `bbox`（不扩容）可正常加载。
- `testout` 版本写入 `pl0al.pzz` 后会无限加载。
- “仅扩容、不改 stream 解压内容”的 `pl0al` 版本也会无限加载。
- 这说明高概率问题在 **`PZZ/AFS` 加载路径对扩容的兼容性**，不是纯几何内容本身。
- 深入 IDA 后新增结论：`Z_DATA.BIN` 的 AFS 读取路径（mode0）按“首偏移 + size链”推导位置，隐含要求后续 entry 物理连续；单独搬移某个 entry 到文件末尾会破坏该假设。

## 关键事实
- 原始 `pl0al.pzz` 大小：`609680`
- testout 回包后 `pl0al.pzz` 大小会增长（> 原始）
- 仅扩容对照包（stream 内容逐字节一致）仍触发无限加载
- 结论方向：优先分析 `PZZ tail`、`descriptor/chunk`、`AFS entry offset/size` 及其运行时校验逻辑
- 新发现：AFS 的 name table（每项 0x30）末尾 `+0x2C` 是文件 size；当前扩容流程只改主表 size，未同步 name table size

## 工作流（每轮实验）
1. 以干净 `Z_DATA.BIN` 为起点。
2. 重新生成 inventory（避免旧 offset/size 污染）。
3. 提取 `pl0al.pzz` 与 streams。
4. 仅做一个变量的改动（单因素实验）。
5. 回包并打入 `Z_DATA.BIN`。
6. 再次生成 inventory，回读 `pl0al.pzz` 做 hash 校验。
7. 游戏实测：记录“不卡/卡死、是否可见变化”。

## 实验矩阵
- [x] A: 微小 `bbox` 改动（不扩容） -> 不卡
- [x] B: testout 全量替换（扩容） -> 卡死
- [x] C: 仅扩容且 stream 解压内容完全不变 -> 卡死
- [x] D: testout 去掉 `m00` mesh（仍扩容） -> 卡死
- [ ] E: 严格不扩容预算下的可见改模（待做）

## 下一步（IDA 逆向优先级）
1. **PZZ tail 校验函数**
   - 找到读取 `PZZ` 后对末尾 16-byte tail 的校验逻辑
   - 确认校验覆盖范围、是否依赖对齐/body size
2. **PZZ chunk/descriptor 解析函数**
   - 找到 stream chunk 遍历与单位大小（128-byte units）处理
   - 确认是否要求“原始 descriptor 不可变化”或有上限
3. **AFS entry 装载函数**
   - 找到按 index 读取 `offset/size` 的调用链
   - 确认是否存在“白名单大小”或“固定地址缓存”
4. **pl0al 资源选择路径**
   - 确认战斗中是否实际读的是 `index=1736/pl0al.pzz`
   - 排除影子资源、缓存路径、变体槽位（例如其他 `pl0a*`）

## IDA 初步定位（已记录）
- `0x88237FC`
  - 作用：AFS magic 检查（比较 `"AFS"`），并读取 header `+0x04` 文件数
  - 关键点：不是 `"AFS"` 就返回错误
- `0x8823A2C`
  - 作用：AFS 状态与分区统计（`adxf_GetPtStat` 路径）
  - 关键点：存在 `"Unacceptable format(not AFS)"`、`"AFS file has 128MB or more"` 等硬错误分支
- `0x88216B8`
  - 作用：`ceil(size / 2048)`，即 2048 对齐块数计算
- `0x8821370`
  - 作用：按分区 + file id 计算文件定位信息（offset/size）
  - 关键点：内部大量依赖 `sub_88216B8` 的 2048 块数累加，属于 AFS 指针计算核心路径
- `0x8823690`
  - 作用：`sub_8821370` 的加锁包装调用

结论：AFS 路径已确认存在严格格式/大小/对齐逻辑，下一步继续定位是否存在“entry 扩容后的附加约束”。

## AFS 结构补充（实测）
- `AFS` 主表：`[offset,size] * file_count`
- 紧随主表的 8 字节：`name_table_offset`, `name_table_size`
- name table 每项 `0x30`：
  - `0x00..0x1F` 文件名
  - `0x20..0x2B` 时间相关字段（3 x u32）
  - `0x2C..0x2F` 文件 size（与主表 size 一致）
- 问题点：扩容后若仅更新主表 size，不更新 name table size，会形成不一致
- 工具修复：`src/afs.rs::patch_afs_entry` 已在扩容分支同步更新 name table `+0x2C` size
- 工具修复补充：当 `new_size <= old_size` 时也同步更新主表/name table size，避免遗留“过大 size + 尾部零填充”状态
- 根因补充：仅更新目标 entry 的 offset/size 并把数据追加到末尾是不够的；需要在 size 变化时整体平移后续数据并批量修正后续 offsets，使布局保持连续。

## IDA 记录模板
- 函数地址：
- 作用：
- 关键输入：
- 关键条件：
- 失败分支：
- 与当前假设关系：

## 操作约束
- 不做自动备份（按当前用户偏好）。
- 每次 `patch-afs` 后必须重建 inventory 再做回读验证。
- 不同时引入多个变量，避免实验结论失真。

## 每轮更新日志
### 2026-04-29
- 建立流程文档。
- 将根因方向收敛到 `PZZ/AFS` 扩容兼容性。
- 新增 IDA AFS 核心函数定位与地址记录。
- 识别到 AFS name table `+0x2C` size 字段未同步更新的高风险问题，并已对 live 进行一次热修复验证。
- 在 `afs.rs` 实现正式修复并完成离线验证：扩容后主表 size 与 name table size 已一致。
- 在 live 文件上完成实测写入校验：`pl0al_testout_repacked.pzz` 与从 live 回读的 `pl0al.pzz` 哈希一致。
- 通过 IDA (`sub_8823A2C/sub_8821370`) 识别出 AFS mode0 的连续布局假设。
- `src/afs.rs` 已升级为“size 变化时重排后续数据 + 修正后续 offsets + 修正 name table 指针/size”的实现。
- 新发现关键细节：AFS 后续偏移推进单位是 `ceil(size/2048)*2048`，不是原始 byte size。
- 修复 `patch_afs_entry`：size 变化时按 2048 对齐区间替换 entry，后续 offsets/name table 偏移按“扇区对齐 delta”平移；并对 entry 尾部自动补零到 2048 对齐。
- 本地一致性校验通过：`zdata_testout_contiguous_v2.bin` 全表满足连续链（`bad_count=0`），且 `pl0al` 主表 size 与 name table `+0x2C` size 一致。
- 强可见验证轮次：重建 live inventory 后确认 `pl0al.pzz` 当前 size 为 `617232`，旧 inventory 存在截断风险。
- 生成 `testout_strong_scale3p5.dae`（36 组 POSITION source 坐标统一放大 3.5 倍），直转得到 `stream000_strong_direct.pmf2`（`175440` bytes）。
- 回包得到 `pl0al_strong_repacked.pzz`（entry size `625168`），并完成 AFS 校验：连续链 `bad_count=0`、主表 size 与 name table `+0x2C` size 一致。
- 已部署 `converted_out/ab6_strong_visible/zdata_strong.bin` 到 `D:\PPSSPP\gundam\PSP_GAME\USRDIR\Z_DATA.BIN`，SHA256 一致。
- 用户反馈：游戏内 `pl0al` 仍无可见变化；回读验证确认已部署包内 `pl0al/stream000.pmf2` 确为强改版（175440 bytes），说明“写入成功但运行时资源路径未命中”的概率上升。
- AB7 轮次：同时对 `pl0a` 与 `pl0al` 的 `stream000/stream002` 做 6x 几何放大重建。
- 结果尺寸：`pl0a stream000=194160`、`pl0a stream002=114448`、`pl0al stream000=175440`、`pl0al stream002=47184`。
- 回包后 entry：`pl0a.pzz=645136`、`pl0al.pzz=628880`；AFS 连续链校验 `bad_count=0`，name table size 同步正常。
- 已部署 `converted_out/ab7_both_pl0a_pl0al/zdata_ab7.bin` 到 `D:\PPSSPP\gundam\PSP_GAME\USRDIR\Z_DATA.BIN`，SHA256 一致。
- AB7 用户反馈：无限加载。
- **隔离实验（AB8/AB9）**：
  - AB8（仅 pl0a，6x scaled，从干净 Z_DATA 单次 patch）→ **卡死**
  - AB9（仅 pl0al，6x scaled，从干净 Z_DATA 单次 patch）→ **不卡**
  - 结论：崩溃源在 `pl0a` 的 PMF2 rebuild 输出，不在 AFS/PZZ 容器层。
- AB10（pl0a 原样回包，不改 stream 内容）→ PZZ 逐字节一致，证明 PZZ repack 无问题。
- XOR key 分析：`derive_xor_key_from_size` 对所有扩容 body size 均产生不同 key，但 AB9（pl0al 扩容）不崩溃，证明游戏不使用此推导——排除 key 问题。
- PMF2 section 结构对比（pl0a orig vs rebuilt）：section count=59 一致，mesh flag 一致，仅 section 0/1 有名称差异。section size 全部变大（预期内，6x 缩放导致 display list 增大）。
- 下一步：确认 AB9 是否产生可见变化（如有，则 pl0al 改造路径已通）；定位 pl0a rebuild 的具体 bug。
