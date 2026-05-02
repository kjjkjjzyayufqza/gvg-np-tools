# 模型修改工作流程（PMF2 Mod 完整指南）

本文档说明如何从 `Z_DATA.BIN` 提取模型、在 Maya 中修改、再打包回游戏的完整流程。

---

## 前置条件

- Rust 工具已编译：`rust_converter/`
- 原始游戏文件：`Z_DATA.BIN`
- Inventory 文件：`data_bin_inventory/Z_DATA.BIN.inventory.json`
- Maya（或其他 DCC）
- Noesis（用于 FBX <-> DAE 转换）

---

## 第一步：从 Z_DATA.BIN 提取 PZZ

```bash
cargo run --manifest-path rust_converter/Cargo.toml -- extract-pzz ^
  "E:/research/gvg_np/Z_DATA.BIN" ^
  "E:/research/gvg_np/data_bin_inventory/Z_DATA.BIN.inventory.json" ^
  --pzz-name pl00.pzz ^
  --out "E:/research/gvg_np/pipeline_out/manual_extract/pl00.pzz"
```

---

## 第二步：从 PZZ 提取 streams

```bash
cargo run --manifest-path rust_converter/Cargo.toml -- extract-streams ^
  "E:/research/gvg_np/pipeline_out/manual_extract/pl00.pzz" ^
  --out "E:/research/gvg_np/pipeline_out/manual_extract/streams"
```

输出目录会包含 `stream000.pmf2`、`stream001.gim` 等文件。

建议立刻备份原始 PMF2：

```bash
copy streams\stream000.pmf2 streams\stream000.pmf2.bak
```

---

## 第三步：PMF2 转 DAE（可导入 Maya）

```bash
cargo run --manifest-path rust_converter/Cargo.toml -- pmf2-to-dae ^
  "E:/research/gvg_np/pipeline_out/manual_extract/streams/stream000.pmf2" ^
  --out "E:/research/gvg_np/pipeline_out/manual_extract/stream000.dae" ^
  --name stream000
```

输出 `stream000.dae`，可直接导入 Maya。

---

## 第四步：在 Maya 中修改

1. 导入 `stream000.dae` 到 Maya
2. 做你需要的修改（缩放骨头、调整位置等）
3. 导出为 FBX：`test.fbx`

---

## 第五步：FBX 转 DAE（用 Noesis）

用 Noesis 打开 `test.fbx`，导出为 `testout.dae`。

---

## 第六步：DAE 转 PMF2（关键步骤）

使用 **模板模式 + 阈值过滤**，基于原始 PMF2 只回写真正改动的骨骼矩阵：

```bash
cargo run --manifest-path rust_converter/Cargo.toml -- dae-to-pmf2 ^
  "E:/research/gvg_np/pipeline_out/manual_extract/testout.dae" ^
  --template-pmf2 "E:/research/gvg_np/pipeline_out/manual_extract/streams/stream000.pmf2.bak" ^
  --matrix-delta-threshold 0.001 ^
  --out "E:/research/gvg_np/pipeline_out/manual_extract/streams/stream000.pmf2"
```

如果你在 Maya 中 **添加了新几何体**（如 pCube1），需要加 `--patch-mesh`：

```bash
cargo run --manifest-path rust_converter/Cargo.toml -- dae-to-pmf2 ^
  "E:/research/gvg_np/pipeline_out/manual_extract/testout.dae" ^
  --template-pmf2 "E:/research/gvg_np/pipeline_out/manual_extract/streams/stream000.pmf2.bak" ^
  --matrix-delta-threshold 0.001 ^
  --patch-mesh ^
  --out "E:/research/gvg_np/pipeline_out/manual_extract/streams/stream000.pmf2"
```

### 参数说明

| 参数 | 作用 |
|------|------|
| `--template-pmf2` | 指定原始 PMF2 作为模板，保留原始二进制结构 |
| `--matrix-delta-threshold 0.001` | 只有骨骼矩阵变化超过此阈值才写入，过滤浮点噪声 |
| `--patch-mesh` | 检测新增网格并重建对应骨骼的 GE 数据（需配合 `--template-pmf2`） |

### 为什么需要模板模式？

DAE 往返链路（PMF2 -> DAE -> Maya -> FBX -> Noesis -> DAE）会引入极小的浮点精度损失（约 `1e-8` 级别）。游戏对骨骼矩阵非常敏感，这些微小变化会导致无限加载。模板模式确保只有你真正修改的骨骼被更新，其余保持原始字节不变。

### 什么时候需要 `--patch-mesh`？

如果你在 DCC 工具中添加了新的网格对象（如 Maya 中创建 pCube 并绑定到某个骨骼），新网格的面数会比原始 PMF2 多。`--patch-mesh` 会检测这种情况，只对面数增加的骨骼重建 GE 数据，其他骨骼保持模板原始字节不变。

---

## 第七步：重打包 PZZ

```bash
cargo run --manifest-path rust_converter/Cargo.toml -- repack-pzz ^
  "E:/research/gvg_np/pipeline_out/manual_extract/pl00.pzz" ^
  "E:/research/gvg_np/pipeline_out/manual_extract/streams" ^
  --out "E:/research/gvg_np/pipeline_out/manual_extract/repacked_pl00.pzz"
```

---

## 第八步：回写 Z_DATA.BIN

用 `pipeline` 的 `patch` 功能，或手动用 Python 脚本将 `repacked_pl00.pzz` 写回 `Z_DATA.BIN` 的对应 entry：

```bash
cargo run --manifest-path rust_converter/Cargo.toml -- extract-pzz ^
  "E:/research/gvg_np/Z_DATA.BIN" ^
  "E:/research/gvg_np/data_bin_inventory/Z_DATA.BIN.inventory.json" ^
  --pzz-name pl00.pzz ^
  --out "E:/research/gvg_np/pipeline_out/manual_extract/pl00.pzz"
```

目前回写 BIN 需要通过脚本完成（将 `repacked_pl00.pzz` 写入 `Z_DATA.BIN` 的 entry 1649 位置）。

---

## 完整流程速查

```
Z_DATA.BIN
  ↓ extract-pzz
pl00.pzz
  ↓ extract-streams
streams/stream000.pmf2  (备份为 .bak)
  ↓ pmf2-to-dae
stream000.dae
  ↓ Maya 导入 → 修改 → 导出 FBX
test.fbx
  ↓ Noesis 转换
testout.dae
  ↓ dae-to-pmf2 --template-pmf2 .bak --matrix-delta-threshold 0.001 [--patch-mesh]
streams/stream000.pmf2  (已更新)
  ↓ repack-pzz
repacked_pl00.pzz
  ↓ 回写 Z_DATA.BIN
Z_DATA_1.BIN  (可进游戏)
```

---

## 注意事项

1. **必须使用 `--template-pmf2`**：直接从 DAE 完全重建的 PMF2 会丢失游戏需要的隐藏数据
2. **必须使用 `--matrix-delta-threshold`**：建议值 `0.001`，过滤 Maya/Noesis 往返产生的浮点噪声
3. **备份原始 PMF2**：修改前务必保留 `.bak`，它是模板模式的基础
4. **PZZ 大小变化**：如果重打包后 PZZ 比原始大，回写 BIN 时会自动追加到文件末尾并更新索引
5. **添加新网格时使用 `--patch-mesh`**：只在 Maya 中添加了新几何体时才需要，仅修改骨骼矩阵时不要加此参数
