use anyhow::Result;
use clap::{Parser, Subcommand};
use gvg_converter::{afs, dae, pmf2, pzz, save::PzzSavePlanner};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "gvg_converter", about = "GVG Next Plus PZZ/PMF2/DAE converter")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    ExtractPzz {
        #[arg(help = "Z_DATA.BIN path")]
        z_bin: PathBuf,
        #[arg(help = "Inventory JSON path")]
        inventory: PathBuf,
        #[arg(long, default_value = "pl00.pzz", help = "PZZ entry name")]
        pzz_name: String,
        #[arg(long, help = "Output path (.pzz file or directory)")]
        out: Option<PathBuf>,
    },
    ExtractStreams {
        #[arg(help = "Input PZZ file")]
        pzz: PathBuf,
        #[arg(long, default_value = "streams_out", help = "Output directory")]
        out: PathBuf,
    },
    ListEntries {
        #[arg(help = "Inventory JSON path")]
        inventory: PathBuf,
        #[arg(long, help = "Case-insensitive name filter")]
        contains: Option<String>,
        #[arg(long, default_value_t = 200, help = "Max rows")]
        limit: usize,
    },
    Pmf2ToDae {
        #[arg(help = "Input PMF2 file")]
        pmf2: PathBuf,
        #[arg(long, help = "Output .dae path or output directory")]
        out: Option<PathBuf>,
        #[arg(long, help = "Model name")]
        name: Option<String>,
    },
    DaeToPmf2 {
        #[arg(help = "Input DAE file")]
        dae: PathBuf,
        #[arg(long, help = "Output PMF2 file")]
        out: Option<PathBuf>,
        #[arg(long, help = "Model name")]
        name: Option<String>,
        #[arg(
            long,
            help = "Use original PMF2 as template and patch bone matrices only"
        )]
        template_pmf2: Option<PathBuf>,
        #[arg(
            long,
            default_value_t = 0.0,
            help = "Only apply matrix updates whose max abs delta exceeds this threshold (template mode)"
        )]
        matrix_delta_threshold: f32,
        #[arg(long, help = "Optional output .pmf2meta.json path")]
        meta_out: Option<PathBuf>,
        #[arg(
            long,
            default_value_t = false,
            help = "Also rebuild GE mesh data for bones with changed geometry (requires --template-pmf2)"
        )]
        patch_mesh: bool,
    },
    RepackPzz {
        #[arg(help = "Original PZZ file (for stream layout and key)")]
        pzz: PathBuf,
        #[arg(help = "Directory containing replacement streamNNN.* files")]
        streams_dir: PathBuf,
        #[arg(long, help = "Output PZZ path")]
        out: Option<PathBuf>,
    },
    Export {
        #[arg(help = "Input PZZ file")]
        pzz: PathBuf,
        #[arg(long, help = "Output directory")]
        out: Option<PathBuf>,
        #[arg(long, help = "Model name")]
        name: Option<String>,
    },
    Import {
        #[arg(help = "Input .pmf2meta.json file")]
        meta: PathBuf,
        #[arg(long, help = "Output PMF2 file")]
        out: Option<PathBuf>,
    },
    Pipeline {
        #[arg(help = "Z_DATA.BIN path")]
        z_bin: PathBuf,
        #[arg(help = "Inventory JSON path")]
        inventory: PathBuf,
        #[arg(long, default_value = "pl00.pzz", help = "PZZ entry name")]
        pzz_name: String,
        #[arg(long, default_value = "pipeline_out_rs", help = "Output directory")]
        out: PathBuf,
        #[arg(long, default_value = "Z_DATA_1.BIN", help = "Output BIN file")]
        output_bin: PathBuf,
    },
    PatchAfs {
        #[arg(help = "Z_DATA.BIN path")]
        z_bin: PathBuf,
        #[arg(help = "Inventory JSON path")]
        inventory: PathBuf,
        #[arg(long, default_value = "pl00.pzz", help = "PZZ entry name")]
        pzz_name: String,
        #[arg(help = "Replacement PZZ file")]
        new_pzz: PathBuf,
        #[arg(long, default_value = "Z_DATA_1.BIN", help = "Output BIN file")]
        out: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::ExtractPzz {
            z_bin,
            inventory,
            pzz_name,
            out,
        } => cmd_extract_pzz(&z_bin, &inventory, &pzz_name, out.as_deref()),
        Commands::ExtractStreams { pzz, out } => cmd_extract_streams(&pzz, &out),
        Commands::ListEntries {
            inventory,
            contains,
            limit,
        } => cmd_list_entries(&inventory, contains.as_deref(), limit),
        Commands::Pmf2ToDae { pmf2, out, name } => {
            cmd_pmf2_to_dae(&pmf2, out.as_deref(), name.as_deref())
        }
        Commands::DaeToPmf2 {
            dae,
            out,
            name,
            template_pmf2,
            matrix_delta_threshold,
            meta_out,
            patch_mesh,
        } => cmd_dae_to_pmf2(
            &dae,
            out.as_deref(),
            name.as_deref(),
            template_pmf2.as_deref(),
            matrix_delta_threshold,
            meta_out.as_deref(),
            patch_mesh,
        ),
        Commands::RepackPzz {
            pzz,
            streams_dir,
            out,
        } => cmd_repack_pzz(&pzz, &streams_dir, out.as_deref()),
        Commands::Export { pzz, out, name } => cmd_export(&pzz, out.as_deref(), name.as_deref()),
        Commands::Import { meta, out } => cmd_import(&meta, out.as_deref()),
        Commands::Pipeline {
            z_bin,
            inventory,
            pzz_name,
            out,
            output_bin,
        } => cmd_pipeline(&z_bin, &inventory, &pzz_name, &out, &output_bin),
        Commands::PatchAfs {
            z_bin,
            inventory,
            pzz_name,
            new_pzz,
            out,
        } => cmd_patch_afs(&z_bin, &inventory, &pzz_name, &new_pzz, &out),
    }
}

fn cmd_extract_pzz(
    z_bin: &std::path::Path,
    inventory_path: &std::path::Path,
    pzz_name: &str,
    out: Option<&std::path::Path>,
) -> Result<()> {
    let inv = afs::load_inventory(inventory_path)?;
    let entry = afs::find_entry_by_name(&inv, pzz_name)
        .ok_or_else(|| anyhow::anyhow!("{} not found in inventory", pzz_name))?;
    let pzz_data = afs::read_entry(z_bin, entry)?;
    let out_path = resolve_pzz_output_path(pzz_name, out)?;
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&out_path, &pzz_data)?;
    eprintln!(
        "Extracted: {} (index={}, offset={}, size={}) -> {}",
        pzz_name,
        entry.index,
        entry.offset,
        entry.size,
        out_path.display()
    );
    Ok(())
}

fn cmd_extract_streams(pzz_path: &std::path::Path, out_dir: &std::path::Path) -> Result<()> {
    let pzz_data = std::fs::read(pzz_path)?;
    let streams = pzz::extract_pzz_streams(&pzz_data);
    if streams.is_empty() {
        anyhow::bail!("No streams found in PZZ");
    }
    reset_output_dir(out_dir)?;
    for (i, s) in streams.iter().enumerate() {
        let ct = pzz::classify_stream(s);
        let ext = match ct {
            "pmf2" => "pmf2",
            "gim" => "gim",
            "sad" => "sad",
            _ => "bin",
        };
        let sp = out_dir.join(format!("stream{:03}.{}", i, ext));
        std::fs::write(&sp, s)?;
        eprintln!("  {}: {} ({} bytes)", i, ct, s.len());
    }
    eprintln!(
        "Extracted {} streams -> {}",
        streams.len(),
        out_dir.display()
    );
    Ok(())
}

fn cmd_list_entries(
    inventory_path: &std::path::Path,
    contains: Option<&str>,
    limit: usize,
) -> Result<()> {
    let inv = afs::load_inventory(inventory_path)?;
    let filter = contains.map(|s| s.to_lowercase());
    let mut shown = 0usize;
    for e in &inv.entries {
        let name = e.name.as_deref().unwrap_or("<unnamed>");
        if let Some(f) = &filter {
            if !name.to_lowercase().contains(f) {
                continue;
            }
        }
        let ext = std::path::Path::new(name)
            .extension()
            .and_then(|x| x.to_str())
            .unwrap_or("");
        eprintln!(
            "#{:4} offset={:<10} size={:<10} ext={:<6} {}",
            e.index, e.offset, e.size, ext, name
        );
        shown += 1;
        if shown >= limit {
            break;
        }
    }
    eprintln!("Listed {} entries", shown);
    Ok(())
}

fn cmd_export(
    pzz_path: &std::path::Path,
    out_dir: Option<&std::path::Path>,
    name: Option<&str>,
) -> Result<()> {
    let model_name = name.unwrap_or_else(|| pzz_path.file_stem().unwrap().to_str().unwrap());
    let out = out_dir.map(PathBuf::from).unwrap_or_else(|| {
        pzz_path
            .parent()
            .unwrap()
            .join(format!("{}_dae", model_name))
    });
    let pzz_data = std::fs::read(pzz_path)?;
    do_export(&pzz_data, &out, model_name)?;
    Ok(())
}

fn cmd_pmf2_to_dae(
    pmf2_path: &std::path::Path,
    out: Option<&std::path::Path>,
    name: Option<&str>,
) -> Result<()> {
    let pmf2_data = std::fs::read(pmf2_path)?;
    let model_name = name.unwrap_or_else(|| pmf2_path.file_stem().unwrap().to_str().unwrap());
    let dae_path = resolve_dae_output_path(model_name, out)?;
    if let Some(parent) = dae_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let meta_path = dae_path.with_extension("pmf2meta.json");

    let (bone_meshes, sections, bbox, _world_mats) =
        pmf2::extract_per_bone_meshes(&pmf2_data, true);
    if bone_meshes.is_empty() {
        anyhow::bail!("No mesh data in PMF2");
    }
    dae::write_dae(&dae_path, &bone_meshes, &sections, model_name)?;
    let meta = pmf2::build_meta(model_name, &sections, bbox, &bone_meshes);
    std::fs::write(&meta_path, serde_json::to_string_pretty(&meta)?)?;

    let total_verts: usize = bone_meshes.iter().map(|bm| bm.vertices.len()).sum();
    let total_faces: usize = bone_meshes.iter().map(|bm| bm.faces.len()).sum();
    eprintln!(
        "DAE: {} ({} verts, {} faces), meta: {}",
        dae_path.display(),
        total_verts,
        total_faces,
        meta_path.display()
    );
    Ok(())
}

fn cmd_dae_to_pmf2(
    dae_path: &std::path::Path,
    out: Option<&std::path::Path>,
    name: Option<&str>,
    template_pmf2: Option<&std::path::Path>,
    matrix_delta_threshold: f32,
    meta_out: Option<&std::path::Path>,
    patch_mesh: bool,
) -> Result<()> {
    let mut meta = dae::read_dae_to_meta(dae_path, name)?;
    if let Some(bbox) = pmf2::compute_auto_bbox_from_bone_meshes(&meta.bone_meshes) {
        meta.bbox = bbox;
    }
    let pmf2_data = if let Some(template_path) = template_pmf2 {
        let template = std::fs::read(template_path)?;
        if patch_mesh {
            let (_, template_bbox) = pmf2::parse_pmf2_sections(&template);
            let needs_larger_bbox = meta
                .bbox
                .iter()
                .zip(template_bbox.iter())
                .any(|(required, current)| required > &(current + 1e-6));
            if needs_larger_bbox {
                eprintln!(
                    "Template bbox too small for imported mesh, expanding bbox while preserving template mesh data"
                );
            }
            pmf2::patch_pmf2_with_mesh_updates(&template, &meta, matrix_delta_threshold)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "failed to patch template PMF2 with DAE transforms: {}",
                        template_path.display()
                    )
                })?
        } else {
            pmf2::patch_pmf2_transforms_from_meta_with_threshold(
                &template,
                &meta,
                matrix_delta_threshold,
            )
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "failed to patch template PMF2 with DAE transforms: {}",
                    template_path.display()
                )
            })?
        }
    } else {
        pmf2::rebuild_pmf2(&meta)
    };
    let out_path = out
        .map(PathBuf::from)
        .unwrap_or_else(|| dae_path.with_extension("pmf2"));
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&out_path, &pmf2_data)?;
    if let Some(meta_path) = meta_out {
        if let Some(parent) = meta_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(meta_path, serde_json::to_string_pretty(&meta)?)?;
    }
    eprintln!(
        "Rebuilt PMF2 from DAE: {} ({} bytes, {} sections, {} bone meshes)",
        out_path.display(),
        pmf2_data.len(),
        meta.sections.len(),
        meta.bone_meshes.len()
    );
    Ok(())
}

fn cmd_repack_pzz(
    original_pzz_path: &std::path::Path,
    streams_dir: &std::path::Path,
    out: Option<&std::path::Path>,
) -> Result<()> {
    let original = std::fs::read(original_pzz_path)?;
    let mut streams = pzz::extract_pzz_streams(&original);
    if streams.is_empty() {
        anyhow::bail!("No streams found in original PZZ");
    }
    let mut replaced = 0usize;
    for (i, stream) in streams.iter_mut().enumerate() {
        if let Some(path) = find_stream_override_path(streams_dir, i)? {
            *stream = std::fs::read(&path)?;
            replaced += 1;
            eprintln!("  replaced stream{:03} <- {}", i, path.display());
        }
    }
    let stream_count = streams.len();
    let rebuilt = PzzSavePlanner::new(&original, streams)
        .plan_preserving_layout()?
        .rebuilt_pzz;
    let out_path = match out {
        Some(p) => p.to_path_buf(),
        None => {
            let stem = original_pzz_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("model.pzz");
            original_pzz_path.with_file_name(format!("rebuilt_{}", stem))
        }
    };
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&out_path, &rebuilt)?;
    eprintln!(
        "Repacked PZZ: streams={} replaced={} -> {}",
        stream_count,
        replaced,
        out_path.display()
    );
    Ok(())
}

fn do_export(pzz_data: &[u8], out: &std::path::Path, model_name: &str) -> Result<ExportResult> {
    let streams = pzz::extract_pzz_streams(pzz_data);
    if streams.is_empty() {
        anyhow::bail!("No streams found in PZZ");
    }

    reset_output_dir(out)?;

    let mut stream_info = Vec::new();
    for (i, s) in streams.iter().enumerate() {
        let ct = pzz::classify_stream(s);
        let ext = match ct {
            "pmf2" => "pmf2",
            "gim" => "gim",
            "sad" => "sad",
            _ => "bin",
        };
        let sp = out.join(format!("stream{:03}.{}", i, ext));
        std::fs::write(&sp, s)?;
        stream_info.push((i, ct.to_string(), s.len()));
    }

    let mut exported_models = Vec::new();
    let pmf2_streams: Vec<(usize, &[u8])> = streams
        .iter()
        .enumerate()
        .filter(|(_, s)| pzz::classify_stream(s) == "pmf2")
        .map(|(i, s)| (i, s.as_slice()))
        .collect();

    for &(pmf2_idx, pmf2_data) in &pmf2_streams {
        let model_file_name = format!("{}_stream{:03}", model_name, pmf2_idx);
        let dae_path = out.join(format!("{}.dae", model_file_name));
        let meta_path = out.join(format!("{}.pmf2meta.json", model_file_name));

        let (bone_meshes, sections, bbox, world_mats) =
            pmf2::extract_per_bone_meshes(pmf2_data, true);
        if bone_meshes.is_empty() {
            eprintln!("  Stream {}: no mesh data", pmf2_idx);
            continue;
        }

        let total_verts: usize = bone_meshes.iter().map(|bm| bm.vertices.len()).sum();
        let total_faces: usize = bone_meshes.iter().map(|bm| bm.faces.len()).sum();

        let _ = world_mats;
        dae::write_dae(&dae_path, &bone_meshes, &sections, &model_file_name)?;

        let meta = pmf2::build_meta(&model_file_name, &sections, bbox, &bone_meshes);
        let meta_json = serde_json::to_string_pretty(&meta)?;
        std::fs::write(&meta_path, &meta_json)?;

        eprintln!(
            "  DAE: {} ({} verts, {} faces)",
            dae_path.display(),
            total_verts,
            total_faces
        );
        exported_models.push(ExportedModel {
            pmf2_stream_index: pmf2_idx,
            dae_path: dae_path.to_string_lossy().to_string(),
            meta_path: meta_path.to_string_lossy().to_string(),
            total_verts,
            total_faces,
            bone_count: sections.len(),
            mesh_parts: bone_meshes.len(),
        });
    }

    let manifest = serde_json::json!({
        "model_name": model_name,
        "stream_count": streams.len(),
        "streams": stream_info.iter().map(|(i, t, s)| serde_json::json!({"index": i, "type": t, "size": s})).collect::<Vec<_>>(),
    });
    std::fs::write(
        out.join("streams_manifest.json"),
        serde_json::to_string_pretty(&manifest)?,
    )?;

    Ok(ExportResult {
        stream_count: streams.len(),
        exported_models,
    })
}

#[allow(dead_code)]
struct ExportedModel {
    pmf2_stream_index: usize,
    dae_path: String,
    meta_path: String,
    total_verts: usize,
    total_faces: usize,
    bone_count: usize,
    mesh_parts: usize,
}

struct ExportResult {
    stream_count: usize,
    exported_models: Vec<ExportedModel>,
}

fn cmd_import(meta_path: &std::path::Path, out: Option<&std::path::Path>) -> Result<()> {
    let text = std::fs::read_to_string(meta_path)?;
    let mut meta: pmf2::Pmf2Meta = serde_json::from_str(&text)?;
    if let Some(bbox) = pmf2::compute_auto_bbox_from_bone_meshes(&meta.bone_meshes) {
        meta.bbox = bbox;
    }
    let pmf2_data = pmf2::rebuild_pmf2(&meta);
    let out_path = out
        .map(PathBuf::from)
        .unwrap_or_else(|| meta_path.with_extension("pmf2"));
    std::fs::write(&out_path, &pmf2_data)?;
    eprintln!(
        "Rebuilt PMF2: {} ({} bytes)",
        out_path.display(),
        pmf2_data.len()
    );
    Ok(())
}

fn cmd_patch_afs(
    z_bin: &std::path::Path,
    inventory_path: &std::path::Path,
    pzz_name: &str,
    new_pzz_path: &std::path::Path,
    output_bin: &std::path::Path,
) -> Result<()> {
    let inv = afs::load_inventory(inventory_path)?;
    let entry = afs::find_entry_by_name(&inv, pzz_name)
        .ok_or_else(|| anyhow::anyhow!("{} not found in inventory", pzz_name))?;
    let new_pzz = std::fs::read(new_pzz_path)?;
    if let Some(parent) = output_bin.parent() {
        std::fs::create_dir_all(parent)?;
    }
    afs::patch_afs_entry(z_bin, entry.index, &new_pzz, output_bin)?;
    Ok(())
}

fn cmd_pipeline(
    z_bin: &std::path::Path,
    inventory_path: &std::path::Path,
    pzz_name: &str,
    out_dir: &std::path::Path,
    output_bin: &std::path::Path,
) -> Result<()> {
    let inv = afs::load_inventory(inventory_path)?;
    let entry = afs::find_entry_by_name(&inv, pzz_name)
        .ok_or_else(|| anyhow::anyhow!("{} not found in inventory", pzz_name))?;

    eprintln!(
        "Target: {} (index={}, offset={}, size={})",
        pzz_name, entry.index, entry.offset, entry.size
    );

    eprintln!("\n=== 1. Extract PZZ ===");
    let pzz_data = afs::read_entry(z_bin, entry)?;
    std::fs::create_dir_all(out_dir)?;
    cleanup_pipeline_artifacts(out_dir, pzz_name)?;
    let pzz_out = out_dir.join(format!("original_{}", pzz_name));
    std::fs::write(&pzz_out, &pzz_data)?;
    eprintln!("  Read {} bytes -> {}", pzz_data.len(), pzz_out.display());

    eprintln!("\n=== 2. Export to DAE ===");
    let export_dir = out_dir.join(format!("{}_dae", pzz_name.replace(".pzz", "")));
    let export_result = do_export(&pzz_data, &export_dir, &pzz_name.replace(".pzz", ""))?;
    eprintln!(
        "  {} streams, {} DAE files",
        export_result.stream_count,
        export_result.exported_models.len()
    );

    eprintln!("\n=== 3. Round-trip: Meta -> PMF2 ===");
    let streams = pzz::extract_pzz_streams(&pzz_data);
    let mut rebuilt_streams: Vec<Vec<u8>> = streams.clone();

    for ef in &export_result.exported_models {
        let meta_text = std::fs::read_to_string(&ef.meta_path)?;
        let meta: pmf2::Pmf2Meta = serde_json::from_str(&meta_text)?;
        let new_pmf2 = pmf2::rebuild_pmf2(&meta);

        let idx = ef.pmf2_stream_index;
        eprintln!(
            "  Stream {}: {} -> {} bytes",
            idx,
            streams[idx].len(),
            new_pmf2.len()
        );
        rebuilt_streams[idx] = new_pmf2;
    }

    eprintln!("\n=== 4. Verify rebuilt PMF2 ===");
    for ef in &export_result.exported_models {
        let idx = ef.pmf2_stream_index;
        let (orig_meshes, orig_secs, _, _) = pmf2::extract_per_bone_meshes(&streams[idx], false);
        let (rebuilt_meshes, rebuilt_secs, _, _) =
            pmf2::extract_per_bone_meshes(&rebuilt_streams[idx], false);
        let orig_faces: usize = orig_meshes.iter().map(|m| m.faces.len()).sum();
        let rebuilt_faces: usize = rebuilt_meshes.iter().map(|m| m.faces.len()).sum();
        eprintln!(
            "  Stream {}: {} bones/{} parts/{} faces -> {} bones/{} parts/{} faces",
            idx,
            orig_secs.len(),
            orig_meshes.len(),
            orig_faces,
            rebuilt_secs.len(),
            rebuilt_meshes.len(),
            rebuilt_faces
        );
    }

    eprintln!("\n=== 5. Repack PZZ ===");
    let new_pzz = PzzSavePlanner::new(&pzz_data, rebuilt_streams)
        .plan_preserving_layout()?
        .rebuilt_pzz;
    let new_pzz_path = out_dir.join(format!("rebuilt_{}", pzz_name));
    std::fs::write(&new_pzz_path, &new_pzz)?;
    eprintln!("  {} -> {} bytes", pzz_data.len(), new_pzz.len());

    let verify_streams = pzz::extract_pzz_streams(&new_pzz);
    eprintln!("  Verification: {} streams", verify_streams.len());
    for (i, s) in verify_streams.iter().enumerate() {
        let ct = pzz::classify_stream(s);
        let orig_ct = if i < streams.len() {
            pzz::classify_stream(&streams[i])
        } else {
            "?"
        };
        let status = if ct == orig_ct { "OK" } else { "MISMATCH" };
        eprintln!("    Stream {}: {} ({} bytes) - {}", i, ct, s.len(), status);
    }

    eprintln!("\n=== 6. Patch Z_DATA.BIN -> {} ===", output_bin.display());
    afs::patch_afs_entry(z_bin, entry.index, &new_pzz, output_bin)?;

    eprintln!("\n=== DONE ===");
    eprintln!("  DAE output:  {}", export_dir.display());
    eprintln!("  Rebuilt PZZ: {}", new_pzz_path.display());
    eprintln!("  New Z_DATA:  {}", output_bin.display());

    Ok(())
}

fn reset_output_dir(dir: &std::path::Path) -> Result<()> {
    if dir.exists() {
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            if path.is_dir() {
                std::fs::remove_dir_all(path)?;
            } else {
                std::fs::remove_file(path)?;
            }
        }
    } else {
        std::fs::create_dir_all(dir)?;
    }
    Ok(())
}

fn resolve_dae_output_path(
    model_name: &str,
    out: Option<&std::path::Path>,
) -> Result<std::path::PathBuf> {
    let path = match out {
        Some(p) => {
            let pstr = p.to_string_lossy().to_ascii_lowercase();
            if pstr.ends_with(".dae") {
                p.to_path_buf()
            } else {
                p.join(format!("{}.dae", model_name))
            }
        }
        None => std::env::current_dir()?.join(format!("{}.dae", model_name)),
    };
    Ok(path)
}

fn find_stream_override_path(
    streams_dir: &std::path::Path,
    index: usize,
) -> Result<Option<std::path::PathBuf>> {
    if !streams_dir.exists() {
        return Ok(None);
    }
    let prefix = format!("stream{:03}.", index);
    for entry in std::fs::read_dir(streams_dir)? {
        let path = entry?.path();
        if !path.is_file() {
            continue;
        }
        let name = match path.file_name().and_then(|s| s.to_str()) {
            Some(v) => v,
            None => continue,
        };
        if name.starts_with(&prefix) {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

fn resolve_pzz_output_path(
    pzz_name: &str,
    out: Option<&std::path::Path>,
) -> Result<std::path::PathBuf> {
    let path = match out {
        Some(p) => {
            let pstr = p.to_string_lossy();
            if pstr.to_ascii_lowercase().ends_with(".pzz") {
                p.to_path_buf()
            } else {
                p.join(pzz_name)
            }
        }
        None => std::env::current_dir()?.join(pzz_name),
    };
    Ok(path)
}

fn cleanup_pipeline_artifacts(out_dir: &std::path::Path, pzz_name: &str) -> Result<()> {
    let original = out_dir.join(format!("original_{}", pzz_name));
    if original.exists() {
        std::fs::remove_file(original)?;
    }

    let rebuilt = out_dir.join(format!("rebuilt_{}", pzz_name));
    if rebuilt.exists() {
        std::fs::remove_file(rebuilt)?;
    }

    let export_dir = out_dir.join(format!("{}_dae", pzz_name.replace(".pzz", "")));
    if export_dir.exists() {
        reset_output_dir(&export_dir)?;
    }

    Ok(())
}
