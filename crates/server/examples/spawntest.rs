fn main() {
    let dir = std::path::Path::new("assets/data");
    let content = deepcore_shared::content::ContentRegistry::load_from_dir(dir).unwrap();
    let spawn = deepcore_shared::worldgen::spawn_position(&content);
    println!("spawn = {:?}", spawn);
    let generator = deepcore_shared::worldgen::WorldGenerator::new(0x0DEE9C04E);
    let mut chunks = deepcore_shared::voxel::LoadedChunks::default();
    for dy in -2..2 { for dz in -2..2 { for dx in -2..2 {
        let c = deepcore_shared::voxel::BlockPos::new(spawn.x+dx, spawn.y+dy, spawn.z+dz).chunk();
        if !chunks.contains(c) { chunks.insert(c, generator.generate_chunk(c, &content)); }
    }}}
    for dy in -4..3 {
        let p = spawn.offset(0, dy, 0);
        let b = chunks.block(p).unwrap_or(999);
        let name = content.block(deepcore_shared::content::ContentId(b)).map(|x| x.key.clone()).unwrap_or("?".into());
        println!("y{} {:?} = {} ({})", dy, p, b, name);
    }
}
