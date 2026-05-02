import re
t = open("pipeline_out_rs/pl00_fbx/pl00_stream000.fbx", encoding="utf-8").read()
geom_names = re.findall(r'Geometry: \d+, "Geometry::(\w+)"', t)
bone_names = re.findall(r'Model: \d+, "Model::(\w+)", "LimbNode"', t)
mesh_names = re.findall(r'Model: \d+, "Model::(\w+)", "Mesh"', t)
print(f"Geometry nodes: {len(geom_names)}")
print(f"  Names: {geom_names[:10]}...")
print(f"Bone (LimbNode) models: {len(bone_names)}")
print(f"Mesh models: {len(mesh_names)}")
print(f"  Names: {mesh_names[:10]}...")
has_skin = "Deformer::Skin" in t
has_cluster = "SubDeformer" in t
print(f"Has Skin deformer: {has_skin}")
print(f"Has Cluster: {has_cluster}")
print(f"ByPolygonVertex: {'ByPolygonVertex' in t}")
print(f"GeometryVersion 124: {'GeometryVersion: 124' in t}")
print(f"File size: {len(t):,} bytes")
