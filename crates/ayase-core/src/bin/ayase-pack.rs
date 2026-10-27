use ayase_core::package::{self, pack, validate};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    io::Cursor,
    path::{Path, PathBuf},
};

fn read_json(p: &Path) -> Result<Value, String> {
    serde_json::from_slice(&fs::read(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn collect(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) -> Result<(), String> {
    for e in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let e = e.map_err(|e| e.to_string())?;
        let p = e.path();
        let ty = e.file_type().map_err(|e| e.to_string())?;
        if ty.is_symlink() {
            return Err("源目录不得含符号链接".into());
        }
        if ty.is_dir() {
            collect(root, &p, out)?
        } else {
            let key = p
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            package::safe_path(&key)?;
            out.insert(key, fs::read(p).map_err(|e| e.to_string())?);
        }
    }
    Ok(())
}
fn save(path: &Path, b: &[u8]) -> Result<(), String> {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p).map_err(|e| e.to_string())?
    }
    fs::write(path, b).map_err(|e| e.to_string())
}
fn wav_ms(b: &[u8]) -> Option<u64> {
    if b.get(..4) != Some(b"RIFF") || b.get(8..12) != Some(b"WAVE") {
        return None;
    }
    let mut pos = 12;
    let mut rate = 0;
    let mut data = 0;
    while pos + 8 <= b.len() {
        let n = u32::from_le_bytes(b[pos + 4..pos + 8].try_into().ok()?) as usize;
        if pos + 8 + n > b.len() {
            break;
        }
        if &b[pos..pos + 4] == b"fmt " && n >= 16 {
            rate = u32::from_le_bytes(b[pos + 16..pos + 20].try_into().ok()?) as u64;
        }
        if &b[pos..pos + 4] == b"data" {
            data += n as u64;
        }
        pos += 8 + n + (n % 2);
    }
    if rate > 0 {
        Some(data * 1000 / rate)
    } else {
        None
    }
}
fn ogg_ms(b: &[u8]) -> Option<u64> {
    let header = b.windows(7).position(|w| w == b"\x01vorbis")?;
    let rate = u32::from_le_bytes(b.get(header + 12..header + 16)?.try_into().ok()?) as u64;
    let mut pos = 0;
    let mut granule = 0;
    while pos + 27 <= b.len() {
        if b.get(pos..pos + 4) != Some(b"OggS") {
            return None;
        }
        let count = b[pos + 26] as usize;
        let sizes = b.get(pos + 27..pos + 27 + count)?;
        let size: usize = sizes.iter().map(|n| *n as usize).sum();
        let g = u64::from_le_bytes(b[pos + 6..pos + 14].try_into().ok()?);
        if g != u64::MAX {
            granule = g
        }
        pos += 27 + count + size;
    }
    if rate > 0 {
        Some(granule * 1000 / rate)
    } else {
        None
    }
}
fn convert(source: &Path, dest: &Path) -> Result<(), String> {
    let sequence = read_json(&source.join("asset/sequence.json"))?;
    let speech = read_json(&source.join("text/zh/speech.json"))?;
    let audio = read_json(&source.join("audio/jp/speech.json"))?;
    let mut files = BTreeMap::new();
    let mut assets = serde_json::Map::new();
    let mut clips = serde_json::Map::new();
    let mut audit = vec![];
    for c in sequence.as_array().ok_or("序列清单不是数组")? {
        let name = c["name"].as_str().ok_or("动作缺少 name")?;
        let relative = c["img"].as_str().ok_or("动作缺少 img")?;
        package::safe_path(relative)?;
        let bytes = fs::read(source.join("asset").join(relative)).map_err(|e| e.to_string())?;
        let (w, h) = package::image_dimensions(&bytes)?;
        let asset_id = format!("visual.{}", relative.replace('/', ".").replace(".webp", ""));
        let target = format!("assets/visual/{}", relative.rsplit('/').next().unwrap());
        assets.insert(
            asset_id.clone(),
            json!({"kind":"image","path":target,"width":w,"height":h}),
        );
        files.insert(target.clone(), bytes);
        let columns = c["frame_num_x"].as_u64().ok_or("缺列数")?;
        let rows = c["frame_num_y"].as_u64().ok_or("缺行数")?;
        let direction = if c["need_reverse"] == true {
            "ping-pong"
        } else if c["origin_reverse"] == true {
            "reverse"
        } else {
            "forward"
        };
        let clip = json!({"asset":asset_id,"layout":{"type":"grid","columns":columns,"rows":rows,"frameCount":columns*rows,"cellWidth":c["frame_size_x"],"cellHeight":c["frame_size_y"]},"frameDurationMs":c["frame_time"].as_f64().unwrap_or(0.1)*1000.0,"direction":direction,"startReverse":c["origin_reverse"]==true,"repeatEndpoints":false,"returnToStart":true,"offset":{"x":c["offset_x"],"y":c["offset_y"]}});
        audit.push(json!({"kind":"animation","id":name,"source":relative,"width":w,"height":h,"frames":columns*rows,"direction":direction,"repeatEndpoints":false,"returnToStart":true,"target":target}));
        clips.insert(name.into(), clip);
    }
    for a in audio.as_array().ok_or("音声清单不是数组")? {
        let name = a["name"].as_str().unwrap();
        let relative = a["audio"].as_str().unwrap();
        package::safe_path(relative)?;
        let target = format!("assets/voice/{relative}");
        match fs::read(source.join("audio").join(relative)) {
            Ok(b) => {
                let ms = wav_ms(&b).or_else(|| ogg_ms(&b));
                audit.push(json!({"kind":"audio","id":name,"source":relative,"durationMs":ms,"target":target,"missing":false}));
                assets.insert(
                    format!("voice.{name}"),
                    json!({"kind":"audio","path":target,"durationMs":ms}),
                );
                files.insert(target, b);
            }
            Err(_) => {
                audit.push(json!({"kind":"audio","id":name,"source":relative,"missing":true}))
            }
        }
    }
    for s in speech.as_array().unwrap() {
        let name = s["name"].as_str().unwrap();
        let text = s["text"]
            .as_str()
            .unwrap_or("")
            .replace("**", "")
            .replace("{nickname}", "你");
        let target = format!("assets/text/{name}.txt");
        assets.insert(format!("text.{name}"), json!({"kind":"text","path":target}));
        files.insert(target, text.into_bytes());
    }
    let model = json!({"schemaVersion":"1.0","logicalSize":{"width":360,"height":280},"anchors":{"feet":{"x":0.5,"y":0.96},"bubble":{"x":0.5,"y":0.05}},"clips":clips,
        "baseModes":{"idle":{"loop":"idle"},"music":{"enter":"music_start","loop":"music_idle","exit":"music_end"},"quiet":{"enter":"silence_start","loop":"silence","exit":"silence_end"}},
        "semanticCues":{"greeting":{"clip":"wave","repeat":1},"reminder":{"clip":"wink","repeat":1},"surprise":{"clip":"amazing","repeat":1}}});
    files.insert(
        "avatar/sprite.json".into(),
        serde_json::to_vec_pretty(&model).unwrap(),
    );
    files.insert("licenses/asset-sources.json".into(),serde_json::to_vec_pretty(&json!({"source":"User-provided local Ema test assets; original manifest author: Cafel","distribution":"Local verification only. Asset redistribution rights are not asserted.","convertedBy":"AyaseCompanion 0.1.0"})).unwrap());
    let windows=vec![("morning","05:00","12:00"),("noon","12:00","18:00"),("evening","18:00","23:00"),("night","23:00","05:00")].into_iter().map(|(id,start,end)|json!({"id":id,"start":start,"end":end,"response":{"mode":"fixed","variants":[{"id":id,"textAsset":format!("text.{id}"),"audioAsset":format!("voice.{id}"),"cue":"greeting"}]}})).collect::<Vec<_>>();
    let manifest = json!({"format":"ayase.character","formatVersion":"1.0","packageId":"local.ema.prototype","version":"0.1.0","displayName":"Ema · 离线验证包","identity":{"characterName":"艾玛","userAddress":"你"},"avatar":{"renderer":"sprite.v1","descriptor":"avatar/sprite.json"},"assets":assets,
        "pluginPresets":[{"presetId":"startup-default","pluginId":"ayase.startup-greeting","configVersion":"1.0","configMode":"complete","enabled":true,"priority":"normal","config":{"source":"app.ready","timeZone":"system","ttlSeconds":300,"windows":windows}},
            {"presetId":"fullscreen-default","pluginId":"ayase.fullscreen-quiet","configVersion":"1.0","enabled":true,"priority":"important","config":{}},
            {"presetId":"media-default","pluginId":"ayase.media-response","configVersion":"1.0","enabled":true,"priority":"normal","config":{}}]});
    let b = pack(files, manifest, "manifest.json")?;
    let validated = validate(&b)?;
    save(dest, &b)?;
    save(
        &dest.with_extension("audit.json"),
        &serde_json::to_vec_pretty(
            &json!({"source":source.to_string_lossy(),"report":validated.report,"resources":audit}),
        )
        .unwrap(),
    )?;
    println!(
        "Converted {} files. SHA256 {}",
        validated.report.file_count, validated.report.hash
    );
    Ok(())
}
fn fixture(dest: &Path) -> Result<(), String> {
    let mut b = Cursor::new(Vec::new());
    let mut im = image::RgbaImage::new(64, 64);
    for (x, y, p) in im.enumerate_pixels_mut() {
        let dx = x as i32 - 32;
        let dy = y as i32 - 32;
        *p = if dx * dx + dy * dy < 26 * 26 {
            image::Rgba([74, 151, 178, 255])
        } else {
            image::Rgba([0, 0, 0, 0])
        }
    }
    image::DynamicImage::ImageRgba8(im)
        .write_to(&mut b, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    let model = json!({"schemaVersion":"1.0","logicalSize":{"width":64,"height":64},"anchors":{"feet":{"x":0.5,"y":0.95},"bubble":{"x":0.5,"y":0.05}},"clips":{"idle":{"asset":"visual.idle","layout":{"type":"single"},"frameDurationMs":1000,"direction":"forward"}},"baseModes":{"idle":{"loop":"idle"}},"semanticCues":{}});
    let files = BTreeMap::from([
        ("assets/idle.png".into(), b.into_inner()),
        (
            "avatar/sprite.json".into(),
            serde_json::to_vec_pretty(&model).unwrap(),
        ),
        ("opaque/future.bin".into(), vec![0, 1, 2, 255]),
    ]);
    let manifest = json!({"format":"ayase.character","formatVersion":"1.0","packageId":"example.static","version":"0.1.0","displayName":"静态验证伙伴","avatar":{"renderer":"sprite.v1","descriptor":"avatar/sprite.json"},"assets":{"visual.idle":{"kind":"image","path":"assets/idle.png"}},"pluginPresets":[{"presetId":"future","pluginId":"unknown.future","configVersion":"7.0","enabled":true,"config":{"opaque":{"nested":[1,"keep",false]},"resource":"opaque/future.bin"}}],"extensions":{"future":true}});
    let bytes = pack(files, manifest, "manifest.json")?;
    validate(&bytes)?;
    save(dest, &bytes)
}
fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().collect();
    let usage =
        "用法: ayase-pack validate FILE | pack DIR OUT | convert-ema SOURCE OUT | fixture OUT";
    match args.get(1).map(String::as_str) {
        Some("validate") => {
            let b = fs::read(args.get(2).ok_or(usage)?).map_err(|e| e.to_string())?;
            println!(
                "{}",
                serde_json::to_string_pretty(&validate(&b)?.report).unwrap()
            );
        }
        Some("pack") => {
            let dir = PathBuf::from(args.get(2).ok_or(usage)?);
            let mut files = BTreeMap::new();
            collect(&dir, &dir, &mut files)?;
            let entry = if files.contains_key("manifest.json") {
                "manifest.json"
            } else {
                "plugin.json"
            };
            let m = package::json_file(&files, entry)?;
            let b = pack(files, m, entry)?;
            validate(&b)?;
            save(Path::new(args.get(3).ok_or(usage)?), &b)?;
        }
        Some("convert-ema") => convert(
            Path::new(args.get(2).ok_or(usage)?),
            Path::new(args.get(3).ok_or(usage)?),
        )?,
        Some("fixture") => fixture(Path::new(args.get(2).ok_or(usage)?))?,
        _ => return Err(usage.into()),
    }
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1)
    }
}
