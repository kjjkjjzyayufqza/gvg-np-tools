import { useState, useRef, useCallback, useMemo, useEffect } from 'react'
import { Canvas, useThree, useLoader } from '@react-three/fiber'
import { OrbitControls, Grid } from '@react-three/drei'
import * as THREE from 'three'

const TEXTURE_MAP = {
  '/models/pl00_assembled_body.obj': '/models/pl00_texture.png',
  '/models/pl00_assembled_full.obj': '/models/pl00_texture.png',
  '/models/pl10_assembled_body.obj': '/models/pl10_texture.png',
  '/models/pl10_assembled_full.obj': '/models/pl10_texture.png',
  '/models/pl41_assembled_body.obj': '/models/pl41_texture.png',
  '/models/pl41_assembled_full.obj': '/models/pl41_texture.png',
}

function parseOBJ(text) {
  const vertices = []
  const normals = []
  const uvs = []
  const objects = []
  let curFaces = []
  let curName = 'default'
  let mtlLib = null

  for (const raw of text.split('\n')) {
    const line = raw.trim()
    if (line.startsWith('v ')) {
      const [, x, y, z] = line.split(/\s+/)
      vertices.push(+x, +y, +z)
    } else if (line.startsWith('vn ')) {
      const [, x, y, z] = line.split(/\s+/)
      normals.push(+x, +y, +z)
    } else if (line.startsWith('vt ')) {
      const [, u, v] = line.split(/\s+/)
      uvs.push(+u, +v)
    } else if (line.startsWith('o ') || line.startsWith('g ')) {
      if (curFaces.length > 0) objects.push({ name: curName, faces: curFaces })
      curName = line.split(/\s+/)[1] || 'default'
      curFaces = []
    } else if (line.startsWith('mtllib ')) {
      mtlLib = line.split(/\s+/)[1]
    } else if (line.startsWith('f ')) {
      const parts = line.split(/\s+/).slice(1)
      const idxs = parts.map(p => {
        const [vi, ti, ni] = p.split('/').map(Number)
        return { v: vi - 1, t: (ti || 0) - 1, n: (ni || 0) - 1 }
      })
      for (let i = 1; i < idxs.length - 1; i++) {
        curFaces.push(idxs[0], idxs[i], idxs[i + 1])
      }
    }
  }
  if (curFaces.length > 0) objects.push({ name: curName, faces: curFaces })

  const allFaces = objects.flatMap(o => o.faces)
  const pos = new Float32Array(allFaces.length * 3)
  const nrm = normals.length > 0 ? new Float32Array(allFaces.length * 3) : null
  const uv = uvs.length > 0 ? new Float32Array(allFaces.length * 2) : null

  for (let i = 0; i < allFaces.length; i++) {
    const f = allFaces[i]
    pos[i * 3] = vertices[f.v * 3]
    pos[i * 3 + 1] = vertices[f.v * 3 + 1]
    pos[i * 3 + 2] = vertices[f.v * 3 + 2]
    if (nrm && f.n >= 0) {
      nrm[i * 3] = normals[f.n * 3]
      nrm[i * 3 + 1] = normals[f.n * 3 + 1]
      nrm[i * 3 + 2] = normals[f.n * 3 + 2]
    }
    if (uv && f.t >= 0) {
      uv[i * 2] = uvs[f.t * 2]
      uv[i * 2 + 1] = uvs[f.t * 2 + 1]
    }
  }

  const geo = new THREE.BufferGeometry()
  geo.setAttribute('position', new THREE.BufferAttribute(pos, 3))
  if (nrm) geo.setAttribute('normal', new THREE.BufferAttribute(nrm, 3))
  if (uv) geo.setAttribute('uv', new THREE.BufferAttribute(uv, 2))
  if (!nrm) geo.computeVertexNormals()
  geo.computeBoundingSphere()

  return {
    geometry: geo,
    vertexCount: vertices.length / 3,
    faceCount: allFaces.length / 3,
    hasUV: uvs.length > 0,
    hasNormals: normals.length > 0,
    objectCount: objects.length,
    objectNames: objects.map(o => o.name),
    mtlLib,
  }
}

function ModelMesh({ geometry, wireframe, texture, showTexture }) {
  const mat = useMemo(() => {
    if (wireframe) {
      return new THREE.MeshBasicMaterial({ color: 0x00ff88, wireframe: true })
    }
    if (showTexture && texture) {
      return new THREE.MeshStandardMaterial({
        map: texture,
        side: THREE.DoubleSide,
        metalness: 0.15,
        roughness: 0.7,
      })
    }
    return new THREE.MeshStandardMaterial({
      color: 0xcc8844,
      side: THREE.DoubleSide,
      flatShading: true,
      metalness: 0.2,
      roughness: 0.6,
    })
  }, [wireframe, texture, showTexture])

  return <mesh geometry={geometry} material={mat} />
}

function FitCamera({ geometry, controlsRef }) {
  const { camera } = useThree()
  useMemo(() => {
    if (!geometry?.boundingSphere) return
    const { center, radius } = geometry.boundingSphere
    const dist = radius * 2.8
    camera.position.set(center.x + dist * 0.3, center.y + dist, center.z + dist * 0.15)
    camera.lookAt(center.x, center.y, center.z)
    camera.up.set(0, 0, -1)
    camera.near = radius * 0.01
    camera.far = radius * 100
    camera.updateProjectionMatrix()
    if (controlsRef.current) {
      controlsRef.current.target.set(center.x, center.y, center.z)
      controlsRef.current.update()
    }
  }, [geometry, camera, controlsRef])
  return null
}

const PRESETS = [
  { group: 'Body + Ornament', items: [
    { url: '/models/pl00_assembled_body.obj', label: 'pl00 RX-78' },
    { url: '/models/pl10_assembled_body.obj', label: 'pl10 Zaku' },
    { url: '/models/pl41_assembled_body.obj', label: 'pl41' },
  ]},
  { group: 'Full (+ weapons)', items: [
    { url: '/models/pl00_assembled_full.obj', label: 'pl00 RX-78 full' },
    { url: '/models/pl10_assembled_full.obj', label: 'pl10 Zaku full' },
    { url: '/models/pl41_assembled_full.obj', label: 'pl41 full' },
  ]},
  { group: 'Skeleton', items: [
    { url: '/models/pl00_assembled_body_skeleton.obj', label: 'pl00 skeleton' },
    { url: '/models/pl10_assembled_body_skeleton.obj', label: 'pl10 skeleton' },
    { url: '/models/pl41_assembled_body_skeleton.obj', label: 'pl41 skeleton' },
  ]},
]

const btnStyle = (active) => ({
  background: active ? '#2563eb' : '#1e293b',
  border: '1px solid #334155',
  borderRadius: 6,
  padding: '5px 12px',
  color: active ? '#fff' : '#94a3b8',
  cursor: 'pointer',
  fontSize: 13,
  fontWeight: active ? 600 : 400,
  transition: 'all 0.15s',
})

export default function App() {
  const [model, setModel] = useState(null)
  const [wireframe, setWireframe] = useState(false)
  const [showTexture, setShowTexture] = useState(true)
  const [texture, setTexture] = useState(null)
  const [info, setInfo] = useState('')
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(false)
  const [selectedUrl, setSelectedUrl] = useState('')
  const inputRef = useRef()
  const controlsRef = useRef()

  const loadTexture = useCallback((url) => {
    if (!url) { setTexture(null); return }
    const loader = new THREE.TextureLoader()
    loader.load(url,
      (tex) => {
        tex.flipY = true
        tex.magFilter = THREE.NearestFilter
        tex.minFilter = THREE.NearestMipmapLinearFilter
        tex.colorSpace = THREE.SRGBColorSpace
        setTexture(tex)
      },
      undefined,
      () => setTexture(null)
    )
  }, [])

  const loadOBJ = useCallback((url) => {
    setLoading(true)
    setError('')
    setSelectedUrl(url)
    fetch(url).then(r => r.text()).then(text => {
      const result = parseOBJ(text)
      if (result.faceCount === 0) { setError('No faces'); setLoading(false); return }
      setModel(result)
      const name = url.split('/').pop()
      setInfo(`${name}: ${result.vertexCount}v, ${result.faceCount}f, ${result.objectCount} parts`)
      setLoading(false)

      const texUrl = TEXTURE_MAP[url]
      if (texUrl) {
        loadTexture(texUrl)
      } else {
        setTexture(null)
      }
    }).catch(err => { setError(err.message); setLoading(false) })
  }, [loadTexture])

  const handleFile = useCallback((e) => {
    const files = Array.from(e.target.files || [])
    const objFile = files.find(f => f.name.endsWith('.obj'))
    const pngFile = files.find(f => f.name.endsWith('.png'))

    if (objFile) {
      setLoading(true)
      setError('')
      setSelectedUrl('')
      const reader = new FileReader()
      reader.onload = (ev) => {
        try {
          const result = parseOBJ(ev.target.result)
          if (result.faceCount === 0) { setError('No faces'); setLoading(false); return }
          setModel(result)
          setInfo(`${objFile.name}: ${result.vertexCount}v, ${result.faceCount}f, ${result.objectCount} parts`)
          setLoading(false)
        } catch (err) { setError(err.message); setLoading(false) }
      }
      reader.readAsText(objFile)
    }

    if (pngFile) {
      const reader = new FileReader()
      reader.onload = (ev) => {
        const loader = new THREE.TextureLoader()
        loader.load(ev.target.result, (tex) => {
          tex.flipY = true
          tex.magFilter = THREE.NearestFilter
          tex.minFilter = THREE.NearestMipmapLinearFilter
          tex.colorSpace = THREE.SRGBColorSpace
          setTexture(tex)
        })
      }
      reader.readAsDataURL(pngFile)
    }
  }, [])

  const handleDrop = useCallback((e) => {
    e.preventDefault()
    const files = Array.from(e.dataTransfer.files || [])
    if (files.length > 0) {
      const dt = new DataTransfer()
      files.forEach(f => dt.items.add(f))
      inputRef.current.files = dt.files
      inputRef.current.dispatchEvent(new Event('change', { bubbles: true }))
    }
  }, [])

  return (
    <div style={{ width: '100vw', height: '100vh', background: '#0f172a', display: 'flex', flexDirection: 'column' }}
         onDragOver={e => e.preventDefault()} onDrop={handleDrop}>

      <div style={{ padding: '10px 20px', background: '#1e293b', color: '#e2e8f0',
                    display: 'flex', alignItems: 'center', gap: 12, flexShrink: 0,
                    borderBottom: '1px solid #334155', flexWrap: 'wrap' }}>
        <span style={{ fontWeight: 700, fontSize: 16, color: '#38bdf8', marginRight: 4 }}>GVG Viewer</span>

        <select value={selectedUrl} onChange={e => { if (e.target.value) loadOBJ(e.target.value) }}
                style={{ background: '#0f172a', border: '1px solid #475569', borderRadius: 6,
                         padding: '5px 10px', color: '#e2e8f0', fontSize: 13 }}>
          <option value="">-- model --</option>
          {PRESETS.map(g => (
            <optgroup key={g.group} label={g.group}>
              {g.items.map(it => <option key={it.url} value={it.url}>{it.label}</option>)}
            </optgroup>
          ))}
        </select>

        <input ref={inputRef} type="file" accept=".obj,.png" multiple onChange={handleFile}
               style={{ display: 'none' }} />
        <button onClick={() => inputRef.current?.click()} style={btnStyle(false)}>
          Open File…
        </button>

        <div style={{ display: 'flex', gap: 6 }}>
          <button onClick={() => setWireframe(false)} style={btnStyle(!wireframe)}>Solid</button>
          <button onClick={() => setWireframe(true)} style={btnStyle(wireframe)}>Wire</button>
        </div>

        {model?.hasUV && (
          <button onClick={() => setShowTexture(t => !t)}
                  style={btnStyle(showTexture && !wireframe)}>
            {texture ? 'Texture' : 'No Tex'}
          </button>
        )}

        {loading && <span style={{ color: '#fbbf24', fontSize: 13 }}>Loading…</span>}
        {info && !loading && <span style={{ color: '#4ade80', fontSize: 12 }}>{info}</span>}
        {error && <span style={{ color: '#f87171', fontSize: 12 }}>{error}</span>}
        {texture && <span style={{ color: '#818cf8', fontSize: 12 }}>🎨 textured</span>}
      </div>

      <div style={{ flex: 1, position: 'relative' }}>
        <Canvas camera={{ fov: 50 }}>
          <color attach="background" args={['#0f172a']} />
          <ambientLight intensity={0.5} />
          <directionalLight position={[5, 10, 5]} intensity={0.9} />
          <directionalLight position={[-5, -3, -5]} intensity={0.35} />
          <directionalLight position={[0, -5, 10]} intensity={0.2} />
          {model && (
            <>
              <ModelMesh
                geometry={model.geometry}
                wireframe={wireframe}
                texture={texture}
                showTexture={showTexture}
              />
              <FitCamera geometry={model.geometry} controlsRef={controlsRef} />
            </>
          )}
          <Grid infiniteGrid fadeDistance={200} fadeStrength={2}
                cellColor="#1e293b" sectionColor="#334155" cellSize={1} sectionSize={5} />
          <OrbitControls ref={controlsRef} makeDefault enableDamping dampingFactor={0.1} />
        </Canvas>

        {!model && (
          <div style={{ position: 'absolute', inset: 0, display: 'flex', alignItems: 'center',
                        justifyContent: 'center', pointerEvents: 'none' }}>
            <div style={{ textAlign: 'center', color: '#475569' }}>
              <div style={{ fontSize: 48, marginBottom: 16 }}>📦</div>
              <div style={{ fontSize: 18, fontWeight: 600 }}>Drop .obj + .png here</div>
              <div style={{ fontSize: 14, marginTop: 8 }}>or select a preset from the menu</div>
            </div>
          </div>
        )}
      </div>
    </div>
  )
}
