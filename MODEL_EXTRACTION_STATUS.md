# 静态模型/资源提取进度说明（截至当前）

工作目录严格限定在 `test/`。

## 当前结论总览

- **容器层**
  - `W_DATA.BIN` / `X_DATA.BIN` / `Y_DATA.BIN` / `Z_DATA.BIN`：均为 `AFS` 容器（`AFS\x00`）。
  - `GETA.BIN`：前 100MB 为全 0（更像运行时缓存/占位文件），对模型提取没有贡献。
- **资源类型（高置信）**
  - `X_DATA.BIN`：`*.adx` 音频为主（与模型无关）。
  - `Y_DATA.BIN`：`PSMF` 视频（与模型无关）。
  - `W_DATA.BIN`：`MWo3` 覆盖层/内存镜像类数据（可用于研究，但不是主要模型包）。
  - `Z_DATA.BIN`：核心内容，包含大量 `*.pzz`（模型/贴图/界面等）。
- **PZZ（关键）**
  - `*.pzz` 存在 **u32 XOR 加密层**（每个文件一个 key）。
  - 解密后数据中存在大量 **zlib 流**（`78 9c` / `78 01` / `78 da` / `78 5e`）。
  - 解压出的子资源里确认存在：
    - **GIM 贴图**：`MIG.00.1PSP`（标准 PSP 纹理格式）。
    - **PMF2 容器**：`PMF2`（自定义模型容器/目录信息）。
    - **疑似 Mesh 顶点数据（int16）**：在大型块中存在可被解释为 `int16 xyz` 的点云候选；当前仅能导出“点云 OBJ”（无三角面），且属于启发式命中，仍需用 PMF2 的结构化解析来最终确认与重建拓扑。

## AFS 容器格式（当前用法）

AFS 解析逻辑在 `test/mwo3lib.py` 的 `parse_afs()` / `afs_extract()`，要点：

- 文件头：`AFS\x00`
- u32 `file_count`
- 紧随其后为 `file_count` 个条目，每个条目 8 字节：`offset(u32) + size(u32)`
- 可选 name table（每条 0x30 字节），用于得到 `pl00.pzz`、`pl00ov5.bin` 等名字

相关脚本：
- `test/afs_inventory.py`
- `test/inventory_all_data_bins.py`
- `test/summarize_inventories.py`
- `test/resource_id_lookup.py`

关键库存文件（已生成）：
- `test/data_bin_inventory/W_DATA.BIN.inventory.json`
- `test/data_bin_inventory/X_DATA.BIN.inventory.json`
- `test/data_bin_inventory/Y_DATA.BIN.inventory.json`
- `test/data_bin_inventory/Z_DATA.BIN.inventory.json`
- `test/data_bin_inventory/_summary.json`

## Z_DATA.BIN / PZZ 的结构与解释方式

### 1) XOR 解密层（u32 key）

对单个 PZZ entry 读取 raw bytes 后，以 u32 为周期异或：

```python
import struct

def xor_dec(data: bytes, key: int) -> bytes:
    out = bytearray(len(data))
    kb = struct.pack("<I", key)
    for i in range(0, len(data) - 3, 4):
        out[i] = data[i] ^ kb[0]
        out[i + 1] = data[i + 1] ^ kb[1]
        out[i + 2] = data[i + 2] ^ kb[2]
        out[i + 3] = data[i + 3] ^ kb[3]
    for i in range((len(data) // 4) * 4, len(data)):
        out[i] = data[i] ^ kb[i % 4]
    return bytes(out)
```

key 的推导采用 “file_count 自洽” 方式（详见 `test/pzz_zlib_harvest.py`）：

- 令 `raw_w0 = u32(raw[0:4])`
- 枚举 `file_count`（典型 2..200）
- 令 `key = raw_w0 ^ file_count`
- 用 `key` 解密前 0x4000 字节，检查 `dec_u32[0] == file_count` 且表后 padding 区域为 0（到 0x800 对齐）

### 2) 解密后的 zlib 流提取

在 **完整解密后的 PZZ bytes** 中扫描所有 zlib 头，并对每个 offset 进行 `zlib.decompressobj(wbits=15)` 解压。

这一步不依赖“按表切块”的假设，能稳定得到 PMF2/GIM/mesh 大块。

脚本实现：
- `test/pzz_zlib_harvest.py`

输出目录：
- `test/pzz_harvest_out/<pzz_name>/stream*.bin|.gim`

### 3) 解压后的子资源格式（已确认）

- **GIM（纹理）**
  - magic：`MIG.00.1PSP`
  - 示例（pl00）：
    - `test/pzz_harvest_out/pl00/stream001_off010f08.gim`
    - `test/pzz_harvest_out/pl00/stream003_off01dd88.gim`
    - `test/pzz_harvest_out/pl00/stream005_off01ea08.gim`
- **PMF2（自定义容器）**
  - magic：`PMF2`
  - 常见出现在 `stream000/stream002/stream004`，例如：
    - `test/pzz_harvest_out/pl00/stream000_off000808.bin`（165,872 bytes）
    - `test/pzz_harvest_out/pl00/stream002_off01d008.bin`（11,136 bytes）
    - `test/pzz_harvest_out/pl00/stream004_off01e908.bin`（384 bytes）
- **Mesh 大块（当前主要来源）**
  - 典型：`stream006`，体积约 900KB+，例如：
    - `test/pzz_harvest_out/pl00/stream006_off01ec88.bin`（931,464 bytes）

## PMF2 结构（当前已知）

通过 `test/parse_pmf2_mesh.py` 对 PMF2 头部做了基础解析：

- `PMF2` + u32 `num_sections`
- `header_size` 常为 `0x20`
- 在 sections 中可见资源名片段（例如 `pl00_m00`）

报告：
- `test/pmf2_mesh_report.json`

## 模型数据（顶点）定位与导出方式

### int16 顶点（关键命中）

对非 `GIM/PMF2` 且体积较大的 block 做 `int16 xyz` 扫描：

- stride 候选：`6, 8, 10, 12, 14, 16, 18, 20, 24, 28, 32`
- shift 扫描：`0..2048`，步进 2
- 判定：样本中绝大多数坐标在合理范围，且非零比例和空间跨度足够大
- 导出：按 `scale=1/256` 写 OBJ 点云（只写 `v x y z`）

重要更正（与你反馈的“OBJ 全部错误/单方向外扩”一致）：

- 之前 `find_i16_mesh.py` 的“扫描判定条件”和“OBJ 导出条件”不一致：扫描阶段会排除明显越界的 `int16`，但导出阶段会把这些越界值也写进 OBJ，导致点云被大量垃圾点污染，视觉上常呈现“往某个方向发散/拉刺”。
- 现已修复：OBJ 导出阶段使用与扫描一致的过滤（范围/非零阈值），导出的点云更接近“候选顶点数据”本身。

脚本与报告：
- `test/find_i16_mesh.py`
- `test/i16_mesh_report.json`

### 已导出的 OBJ（重点）

以下 OBJ 均位于 `test/pzz_harvest_out/`，文件名后缀为 `.i16mesh.obj`：

- **pl00（机体）**
  - `pl00/stream006_off01ec88.i16mesh.obj`：32,484 vertices
  - `pl00/stream007_off0bc808.i16mesh.obj`：1,870 vertices
  - `pl00/stream008_off0bd308.i16mesh.obj`：185 vertices（高度退化，极可能不是顶点位置）
- **pl00l（LOD/变体）**
  - `pl00l/stream006_off01b188.i16mesh.obj`：32,484 vertices
  - `pl00l/stream007_off0b8d08.i16mesh.obj`：1,870 vertices
  - `pl00l/stream008_off0b9808.i16mesh.obj`：185 vertices（高度退化，极可能不是顶点位置）
- **pl10（机体）**
  - `pl10/stream007_off0b4788.i16mesh.obj`：2,300 vertices
  - `pl10/stream008_off0b5608.i16mesh.obj`：471 vertices（高度退化，极可能不是顶点位置）
  - `pl10/stream010_off0b5e88.i16mesh.obj`：680 vertices
- **pl41（机体）**
  - `pl41/stream006_off029008.i16mesh.obj`：37,087 vertices
  - `pl41/stream007_off0ac688.i16mesh.obj`：1,537 vertices
  - `pl41/stream008_off0ad088.i16mesh.obj`：439 vertices（高度退化，极可能不是顶点位置）
- **dm00（地图/场景候选）**
  - `dm00/stream001_off084208.i16mesh.obj`：716 vertices
  - `dm00/stream002_off084888.i16mesh.obj`：309 vertices
- **basic（基础资源集合）**
  - `basic/stream006_off02fb08.i16mesh.obj`：1,416 vertices

### 关于 `*.mesh.obj` / `*.obj`（强烈提示）

- `analyze_pzz_blocks.py` 会对 `stream*.bin` 做 float32 “盲探测”，并在命中时导出 `*.mesh.obj`。这类导出非常容易误把“角度表/矩阵/参数表/索引表”当成顶点坐标，因此 **不能** 作为“已提取模型”的证据。
- `pzz_harvest_out` 目录中存在的“裸 `*.obj`”（不带 `.mesh.obj/.point_cloud.obj/.i16mesh.obj` 后缀）属于非结构化点云伪结果，数值形态常呈现轴向退化与离群点，建议视为历史实验产物并忽略。

## 已跑通的代表性 PZZ 样本

数据来源：`test/pzz_harvest_report.json`

- `pl00.pzz`
  - **key**：`0xa268052a`
  - **file_count(fc)**：12
  - **zlib streams**：11
  - **total_decompressed**：1,232,980 bytes
  - 输出目录：`test/pzz_harvest_out/pl00/`
- `pl00l.pzz`
  - **key**：`0x1cd56d68`
  - **fc**：12
  - 输出目录：`test/pzz_harvest_out/pl00l/`
- `pl10.pzz`
  - **key**：`0x59aaee75`
  - **fc**：12
  - 输出目录：`test/pzz_harvest_out/pl10/`
- `pl41.pzz`
  - **key**：`0x1cd56d68`
  - **fc**：12
  - 输出目录：`test/pzz_harvest_out/pl41/`

## 误报/旁支结论

- `Z_DATA.BIN` 全文件扫描曾命中一次 `GMO\x00`，定位后发现落在 `bgm18.bgm` 内部，不代表存在标准 `GMO` 模型文件。
- `GETA.BIN` 的“全 0”现象与社区描述一致（更像缓存/占位）。

## 相关脚本清单（test/）

### 数据盘/AFS 盘点

- `afs_inventory.py`
- `inventory_all_data_bins.py`
- `summarize_inventories.py`
- `resource_id_lookup.py`

### PZZ 分析/解密/解压演进

- `pzz_zlib_harvest.py`：当前稳定方案（产出 `pzz_harvest_out/`）

### Mesh/PMF2 解析与导出

- `analyze_pzz_blocks.py`：对每个 stream 进行格式归类（PMF2/GIM/顶点候选）
- `parse_pmf2_mesh.py`：PMF2 头部与大块分析（基础）
- `find_i16_mesh.py`：int16 顶点扫描与 `.i16mesh.obj` 导出（关键）

## 复现实验（命令）

在仓库根目录执行：

```bash
python test/pzz_zlib_harvest.py
python test/find_i16_mesh.py
```

调试模式（导出未过滤原始顶点，便于在 Blender 中对比验证）：

```bash
python test/find_i16_mesh.py --debug
```

会额外生成 `*.i16mesh.raw.obj`，包含所有按 stride/shift 解析出的顶点（无过滤）。导入 Blender 后若形状不像高达，则说明解析假设有误。

## IDA 逆向辅助结论（EBOOT 分析）

通过 IDA Pro 对 PSP EBOOT 的交叉引用分析：

- `Z_DATA.BIN` / `W_DATA.BIN` / `X_DATA.BIN` / `Y_DATA.BIN` 字符串位于 0x8a1bcfc 附近，为静态表。
- 资源加载 `sub_8886EA4(res_id, ...)`：资源大小取自 `dword_8A56160[res_id & 0x7FFF] + 16`，`+16` 与 PZZ 头部假设一致。
- ZLIB 解压由 `sub_88627F0` 初始化，缓冲区约 98KB。
- AFS 解析使用 adxf_* / adxt_* 系列接口。

后续可结合 `resource_id_lookup.py` 将 IDA 中的 `res_id` 与 AFS 条目对应，验证 PZZ 解密与 zlib 流切分逻辑。

## PPSSPP 帧转储（.ppdmp）辅助分析（推荐路径）

当“盲扫顶点”无法得到高达轮廓时，PPSSPP 的 `创建帧转储` 输出（如 `NPJH50107_0001.ppdmp`）可以直接告诉我们：
- 帧内哪些 VADDR（顶点缓冲地址）被频繁使用
- 对应数据在 dump payload 中的布局特征（例如 stride/shift）

当前已接入的脚本（仅用于调试正确性，产出 Blender 可导入点云 OBJ）：

- `test/ppdmp_vaddr_scan.py`
  - 解析 `.ppdmp` 的 zstd payload
  - 统计 VADDR 频次
  - 对 top VADDR 做 `int16 xyz` 扫描并导出 `*.obj` / `*.raw.obj`

示例命令：

```bash
python test/ppdmp_vaddr_scan.py ^
  --ppdmp test/ppsspp_dump/NPJH50107_0001.ppdmp ^
  --out-dir test/ppsspp_dump/vaddr_scan_out ^
  --top 25
```

输出目录示例：`test/ppsspp_dump/vaddr_scan_out/`

## 当前能力边界

- 已稳定拿到：贴图（GIM）与大量顶点点云（int16 xyz）。
- 仍未完成：三角形索引、材质绑定、骨骼/权重、动画等（大概率在 PMF2 表/小块中，需要继续结构化解析）。

