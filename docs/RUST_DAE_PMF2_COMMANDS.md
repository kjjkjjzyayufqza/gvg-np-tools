# Rust 命令速查（PMF2 / DAE / 重打包）

以下命令基于当前项目的 Rust 工具位于仓库根目录（`Cargo.toml`）。

## 0. 先确认可用子命令

```bash
cargo run --bin gvg_converter -- --help
```

你应该能看到这些命令：

- `pmf2-to-dae`
- `dae-to-pmf2`
- `import`
- `repack-pzz`
- `pipeline`
- 以及 `extract-pzz` / `extract-streams` / `list-entries`

---

## 1) `stream000.pmf2` 转 `dae`

```bash
cargo run --bin gvg_converter -- pmf2-to-dae "E:/research/gvg_np/pipeline_out/manual_extract/streams/stream000.pmf2" --out "E:/research/gvg_np/pipeline_out/manual_extract/stream000.dae" --name stream000
```

输出：

- `stream000.dae`
- `stream000.pmf2meta.json`

`pmf2meta.json` 会在后续 PMF2 重建时使用。

---

## 2) `dae` 转 `pmf2` 用什么命令

现在支持直接解析 DAE 并生成 PMF2：

```bash
cargo run --bin gvg_converter -- dae-to-pmf2 "E:/research/gvg_np/pipeline_out/manual_extract/testout.dae" --out "E:/research/gvg_np/pipeline_out/manual_extract/stream000_dae_direct.pmf2" --meta-out "E:/research/gvg_np/pipeline_out/manual_extract/stream000_dae_direct.pmf2meta.json"
```

说明：

- `--meta-out` 可选，用于导出解析后的 `pmf2meta.json` 方便检查。
- `--name` 可选，用于指定模型名（默认用 DAE 文件名）。
- `--patch-mesh` 配合 `--template-pmf2` 使用，检测 DAE 中新增的网格（如 pCube1）并重建对应骨骼的 GE 数据，其余骨骼保持模板原始字节。
- 旧流程 `import <pmf2meta.json>` 依然可用，适合已有 meta 的情况。

---

## 3) 重新打包 `.pzz` 用什么命令

把 `streamNNN.*` 目录里的流重打包回 PZZ：

```bash
cargo run --bin gvg_converter -- repack-pzz "E:/research/gvg_np/pipeline_out/manual_extract/pl00.pzz" "E:/research/gvg_np/pipeline_out/manual_extract/streams" --out "E:/research/gvg_np/pipeline_out/manual_extract/repacked_pl00.pzz"
```

这个命令会：

- 读取原始 `pl00.pzz` 的流布局和 key
- 用 `streams` 目录中同名 `streamNNN.*` 覆盖对应流
- 生成新的 `repacked_pl00.pzz`

---

## 4) 如果要直接回写成新的 `Z_DATA.BIN`

用 `pipeline`：

```bash
cargo run --bin gvg_converter -- pipeline "E:/research/gvg_np/Z_DATA.BIN" "E:/research/gvg_np/data_bin_inventory/Z_DATA.BIN.inventory.json" --pzz-name pl00.pzz --out "E:/research/gvg_np/pipeline_out_rs" --output-bin "E:/research/gvg_np/pipeline_out_rs/Z_DATA_1.BIN"
```

这条命令会完整执行提取、导出、重建、重打包和 BIN 打补丁。
