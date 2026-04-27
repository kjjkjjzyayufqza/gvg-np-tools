# GVG Next Plus 模型提取指南

从零开始，将 Gundam VS Gundam Next Plus (PSP) 的 `plXX.pzz` 文件转换为带骨骼的 3D 模型。

## 前置条件

- Python 3.10+
- Pillow (`pip install Pillow`)
- 游戏数据文件 `Z_DATA.BIN` 及其索引 `data_bin_inventory/Z_DATA.BIN.inventory.json`

## 文件结构概览

```
Z_DATA.BIN (AFS archive)
  └─ pl00.pzz (XOR 加密)
       └─ zlib 解压后得到多个 stream:
            stream 0: PMF2 (主模型 — 骨骼 + GE显示列表 + 顶点数据)
            stream 1: GIM  (256×256 纹理)
            stream 2: PMF2 (特效/武器模型)
            stream 3: GIM  (128×128 纹理)
            stream 4: PMF2 (元数据)
            stream 5: GIM  (小纹理)
            stream 6: SAD  (骨骼动画数据)
            stream 7+: 动画元数据
```

## 快速开始

### 1. 批量导出全部玩家模型

```bash
python gvg_converter.py batch Z_DATA.BIN data_bin_inventory/Z_DATA.BIN.inventory.json \
    --out converted_out --filter pl
```

输出目录结构：
```
converted_out/pl00/
  ├── pl00_m00.obj          # 主体模型 (body + ornament, OBJ格式)
  ├── pl00_m00.mtl          # 材质文件 (引用纹理PNG)
  ├── stream001.gim         # 原始GIM纹理
  ├── stream001.png         # 转换后的PNG纹理
  ├── stream003.png         # 第二张纹理
  ├── stream005.png         # 第三张纹理
  └── conversion_report.json
```

### 2. 在 3D 查看器中预览

```bash
cd viewer
npm install
npm run dev
```

打开浏览器访问 `http://localhost:5173/`，从下拉菜单选择模型。
也可以将 `converted_out/` 中的 `.obj` 文件拖拽到查看器中打开。

## 数据格式详解

### PMF2 文件结构

```
偏移      内容
0x00      magic "PMF2" (4 bytes)
0x04      section 数量 (u32)
0x08      header 大小 (u32)
0x10      bbox 半径 X (float) ← 也是顶点缩放因子
0x14      bbox 半径 Y (float)
0x18      bbox 半径 Z (float)
0x20      offset table: 每个 section 的文件偏移 (u32 × N)
```

### PMF2 Section Header (每个 256 bytes)

每个 section 代表一个骨骼节点，可能附带 mesh 数据。

```
偏移      大小    内容
+0x00     64B     4×4 float 矩阵 (local bind transform, row-major)
+0x40     32B     8 float: [f0,f1,f2,f3, sx,sy,sz,0] (动画参数 + 缩放标记)
+0x60     16B     名称字符串 (如 "pl00_m02")
+0x70     4B      flags
+0x7C     4B      parent bone index (0xFFFFFFFF = 根节点)
+0xC0     ~64B    子骨骼索引列表 (0xFF 填充)
+0x100    ...     GE 显示列表 + 顶点/索引数据 (如果有 mesh)
```

### 命名规则

| 前缀 | 含义 | 示例 |
|------|------|------|
| `_m` | 身体部件 | m00=root, m02=torso, m04/m08=上臂, m13-m18=腿 |
| `_o` | 装饰部件 | 肩甲、翅膀、裙甲 |
| `_w` | 武器 | 光束步枪、盾牌 |
| `_z` | 特效 | 推进器火焰、光束 |

### 骨骼层级 (pl00 RX-78 示例)

```
m00 (root)
  └─ m01 (spine, Y=12.7)
       ├─ m02 (torso) ─┬─ m03 ── m04 (L上臂) ── m05 (L前臂) ── m06 (L手)
       │                ├─ m07 ── m08 (R上臂) ── m09 (R前臂) ── m10 (R手)
       │                ├─ m11 (head)
       │                └─ o00-o08 (装饰件)
       └─ m12 (hip) ─┬─ m13 (L大腿) ── m14 (L小腿) ── m15 (L脚)
                      └─ m16 (R大腿) ── m17 (R小腿) ── m18 (R脚)
```

## 核心算法

### 1. PZZ 解密

```python
# XOR key 推导: 假设第一个 u32 解密后等于 stream 数量
raw_word0 = read_u32(pzz_data, 0)
for file_count in range(2, 200):
    key = raw_word0 ^ file_count
    # 验证: 解密后的 offset table 末尾是否为 0 填充
```

### 2. 骨骼世界矩阵

每个 section 的 +0x00 矩阵是**局部变换** (parent-relative)。
世界矩阵通过父子链相乘得到：

```python
def world_matrix(bone_index):
    local = sections[bone_index].local_matrix   # +0x00 处的 4×4 矩阵
    parent = sections[bone_index].parent         # +0x7C
    if parent == root:
        return local
    return mat4_multiply(local, world_matrix(parent))  # row-major: child × parent
```

矩阵格式 (row-major, translation 在最后一行):
```
[R00  R01  R02  0]
[R10  R11  R12  0]
[R20  R21  R22  0]
[Tx   Ty   Tz   1]
```

### 3. 顶点缩放 (关键!)

PSP 顶点用 `int16` 存储，范围 [-32768, 32767]。
**缩放因子 = bbox 半径 / 32768**，每个轴独立：

```python
sx = bbox_x / 32768.0   # 例: 4.032 / 32768 = 0.000123
sy = bbox_y / 32768.0   # 例: 22.329 / 32768 = 0.000681
sz = bbox_z / 32768.0   # 例: 4.032 / 32768 = 0.000123
```

完整顶点变换：
```python
# raw_vertex: int16 从 GE 显示列表读取
scaled = (raw_x * sx, raw_y * sy, raw_z * sz)
world_pos = transform_point(world_matrix, scaled)
# world_pos = scaled · rotation_part + translation
```

### 4. GE 显示列表解析

每个有 mesh 的 section 在 +0x100 之后包含 PSP GE 命令序列：

```
ORIGIN (0x14)     → 设置基地址 (所有偏移相对于此)
BASE   (0x10)     → 地址高位
IADDR  (0x02)     → 索引缓冲偏移
VADDR  (0x01)     → 顶点缓冲偏移
VERTEXTYPE (0x12) → 顶点格式 (位域: tc, color, normal, pos, index, weight)
PRIM   (0x04)     → 绘制命令 (type=4 表示 triangle strip, 低16位=顶点数)
RET    (0x0B)     → 返回
```

常见顶点格式：
| VTYPE | 组成 | Stride |
|-------|------|--------|
| 0x000942 | TC_16bit + Normal_16bit + Pos_16bit + Idx_8bit | 16B |
| 0x001142 | TC_16bit + Normal_16bit + Pos_16bit + Idx_16bit | 16B |
| 0x000102 | TC_16bit + Pos_16bit (无法线) | 10B |

### 5. Triangle Strip → 三角面

PSP 使用 triangle strip 节省内存。转换为三角面时需要交替翻转 winding：

```python
for i in range(len(strip) - 2):
    if degenerate(strip[i], strip[i+1], strip[i+2]):
        flip = False   # 退化三角形重置翻转
        continue
    if flip:
        triangle(strip[i+1], strip[i], strip[i+2])
    else:
        triangle(strip[i], strip[i+1], strip[i+2])
    flip = not flip
```

### 6. GIM 纹理

GIM 文件 (`MIG.00.1PSP` magic) 使用层次化 block 结构：
- Block 0x04 (Image): 包含像素数据、宽高、格式
- Block 0x05 (Palette): 包含 CLUT 调色板 (用于 index4/index8 格式)

PSP 纹理常使用 **swizzle** 优化内存访问，解码时需要 unswizzle。

## 坐标系

| 空间 | X | Y | Z |
|------|---|---|---|
| 游戏 (PSP GE) | 左右 | 上下 (高度) | 前后 |
| OBJ 导出 | 左右 | 前后 | 上下 (翻转) |

导出时做 Y↔Z 交换：`obj_xyz = (game_x, game_z, -game_y)`

## 项目文件说明

| 文件 | 用途 |
|------|------|
| `gvg_converter.py` | 主转换器: PZZ→PMF2→骨骼组装→OBJ+MTL |
| `gim_converter.py` | GIM 纹理→PNG |
| `pzz_zlib_harvest.py` | PZZ 解密+流提取 (独立工具) |
| `afs_inventory.py` | AFS 档案索引生成 |
| `inventory_all_data_bins.py` | 批量生成所有 DATA.BIN 索引 |
| `mwo3lib.py` | 共享工具库 (AFS解析等) |
| `viewer/` | Three.js 3D 模型查看器 |
