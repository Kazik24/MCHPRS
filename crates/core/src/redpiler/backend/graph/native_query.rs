use super::*;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

const MAX_NATIVE_LINKS: usize = 256;
static NEXT_ARTIFACT: AtomicU64 = AtomicU64::new(0);

type NativeQueryFn = unsafe extern "C" fn(u32, *const u8) -> u8;

#[derive(Clone, Debug)]
pub struct NativeQueryStats {
    pub compile_time: Duration,
    pub total_targets: usize,
    pub compiled_targets: usize,
    pub compiled_links: usize,
    pub hits: u64,
    pub fallbacks: u64,
    pub source: PathBuf,
    pub library: PathBuf,
}

#[derive(Debug)]
pub struct NativeQueryError(String);

impl NativeQueryError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl std::fmt::Display for NativeQueryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for NativeQueryError {}

struct QueryPlan {
    id: u32,
    attenuations: Vec<u8>,
}

pub(super) struct NativePowerQueries {
    _library: libloading::Library,
    query: NativeQueryFn,
    target_ids: FxHashMap<PowerTarget, u32>,
    stats: NativeQueryStats,
    hits: AtomicU64,
    fallbacks: AtomicU64,
}

impl NativePowerQueries {
    pub(super) fn compile(
        program: &CircuitProgram,
        clang: &Path,
        linker: Option<&Path>,
        artifact_dir: &Path,
    ) -> Result<Self, NativeQueryError> {
        let started = Instant::now();
        let mut grouped = BTreeMap::<PowerTarget, Vec<PowerLink>>::new();
        for &link in program.power_links() {
            grouped.entry(link.to).or_default().push(link);
        }

        let mut plans = Vec::new();
        let mut target_ids = FxHashMap::default();
        let mut compiled_links = 0;
        for (target_index, (target, links)) in grouped.iter().enumerate() {
            let Some(attenuations) = supported_attenuations(links, program.sites(), program.guards()) else {
                continue;
            };
            let id = u32::try_from(target_index)
                .map_err(|_| NativeQueryError("too many power query targets for native demo".into()))?;
            compiled_links += attenuations.len();
            target_ids.insert(*target, id);
            plans.push(QueryPlan { id, attenuations });
        }
        if plans.is_empty() {
            return Err(NativeQueryError("lowered graph has no power queries supported by the native demo".into()));
        }

        fs::create_dir_all(artifact_dir).map_err(|error| NativeQueryError(format!("create native artifact directory: {error}")))?;
        let unique = NEXT_ARTIFACT.fetch_add(1, Ordering::Relaxed);
        let stem = format!("power-query-{}-{unique}", std::process::id());
        let source = artifact_dir.join(format!("{stem}.c"));
        #[cfg(windows)]
        let library = artifact_dir.join(format!("{stem}.dll"));
        #[cfg(not(windows))]
        let library = artifact_dir.join(format!("{stem}.so"));
        let generated = generate_c(&plans);
        fs::write(&source, generated).map_err(|error| NativeQueryError(format!("write generated C source: {error}")))?;

        #[cfg(windows)]
        compile_windows(clang, linker.ok_or_else(|| NativeQueryError("Windows native demo needs an lld-link path".into()))?, &source, &library)?;
        #[cfg(not(windows))]
        compile_unix(clang, &source, &library)?;

        let loaded = unsafe { libloading::Library::new(&library) }
            .map_err(|error| NativeQueryError(format!("load generated native library: {error}")))?;
        let query = unsafe { *loaded.get::<NativeQueryFn>(b"mchprs_query\0") }
            .map_err(|error| NativeQueryError(format!("find generated query function: {error}")))?;
        let compiled_targets = plans.len();
        let stats = NativeQueryStats {
            compile_time: started.elapsed(),
            total_targets: grouped.len(),
            compiled_targets,
            compiled_links,
            hits: 0,
            fallbacks: 0,
            source,
            library,
        };
        Ok(Self { _library: loaded, query, target_ids, stats, hits: AtomicU64::new(0), fallbacks: AtomicU64::new(0) })
    }

    pub(super) fn target_id(&self, target: PowerTarget) -> Option<u32> {
        self.target_ids.get(&target).copied()
    }

    pub(super) fn evaluate(&self, target_id: u32, powers: &[u8]) -> u8 {
        self.hits.fetch_add(1, Ordering::Relaxed);
        unsafe { (self.query)(target_id, powers.as_ptr()) }
    }

    pub(super) fn fallback(&self) {
        self.fallbacks.fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn stats(&self) -> NativeQueryStats {
        let mut stats = self.stats.clone();
        stats.hits = self.hits.load(Ordering::Relaxed);
        stats.fallbacks = self.fallbacks.load(Ordering::Relaxed);
        stats
    }
}

fn supported_attenuations(links: &[PowerLink], sites: &[SiteSpec], guards: &[Guard]) -> Option<Vec<u8>> {
    if links.is_empty() || links.len() > MAX_NATIVE_LINKS {
        return None;
    }
    for link in links {
        if guards.get(link.guard.index()) != Some(&Guard::Always)
            || !matches!(link.from_port, PowerPort::Output | PowerPort::Stored)
            || !matches!(sites.get(link.from.index())?.source, SiteSource::Component(_))
        {
            return None;
        }
    }
    Some(links.iter().map(|link| link.attenuation).collect())
}

fn generate_c(plans: &[QueryPlan]) -> String {
    let mut source = String::from(
        "typedef unsigned char u8; typedef unsigned int u32;\n#if defined(_WIN32)\n#define EXPORT __declspec(dllexport)\n#else\n#define EXPORT __attribute__((visibility(\"default\")))\n#endif\n",
    );
    for (index, plan) in plans.iter().enumerate() {
        source.push_str(&format!("static u8 query_{index}(const u8 *p) {{ u8 m = 0;\n"));
        for (slot, attenuation) in plan.attenuations.iter().enumerate() {
            source.push_str(&format!(
                "if (p[{slot}] > {attenuation}) {{ u8 v = (u8)(p[{slot}] - {attenuation}); if (v > m) m = v; }}\n"
            ));
        }
        source.push_str("return m; }\n");
    }
    source.push_str("EXPORT u8 mchprs_query(u32 target, const u8 *p) { switch (target) {\n");
    for (index, plan) in plans.iter().enumerate() {
        source.push_str(&format!("case {}: return query_{index}(p);\n", plan.id));
    }
    source.push_str("default: return 0; } }\n");
    source
}

#[cfg(windows)]
fn compile_windows(clang: &Path, linker: &Path, source: &Path, library: &Path) -> Result<(), NativeQueryError> {
    if std::env::consts::ARCH != "x86_64" {
        return Err(NativeQueryError("the Windows native demo currently targets x86_64".into()));
    }
    let object = library.with_extension("obj");
    run_tool(
        Command::new(clang)
            .args(["--target=x86_64-pc-windows-msvc", "-O3", "-c"])
            .arg(source)
            .arg("-o")
            .arg(&object),
        "Clang C compilation",
    )?;
    let result = run_tool(
        Command::new(linker)
            .args(["/dll", "/noentry", "/nodefaultlib", "/machine:x64", "/export:mchprs_query"])
            .arg(format!("/out:{}", library.display()))
            .arg(&object),
        "lld-link DLL creation",
    );
    let _ = fs::remove_file(object);
    result
}

#[cfg(not(windows))]
fn compile_unix(clang: &Path, source: &Path, library: &Path) -> Result<(), NativeQueryError> {
    run_tool(
        Command::new(clang)
            .args(["-O3", "-shared", "-fPIC"])
            .arg(source)
            .arg("-o")
            .arg(library),
        "Clang shared-library creation",
    )
}

fn run_tool(command: &mut Command, name: &str) -> Result<(), NativeQueryError> {
    let output = command.output().map_err(|error| NativeQueryError(format!("start {name}: {error}")))?;
    if !output.status.success() {
        return Err(NativeQueryError(format!(
            "{name} failed ({}): {}{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mchprs_blocks::BlockPos;

    fn link(guard: u32, port: PowerPort, attenuation: u8) -> PowerLink {
        PowerLink {
            from: SiteId::new(0),
            from_port: port,
            to: PowerTarget::Input { node: NodeId::new(0), port: InputPort::Main },
            face: mchprs_blocks::BlockFace::Top,
            attenuation,
            guard: GuardId::new(guard),
        }
    }

    fn source_site(source: SiteSource) -> SiteSpec {
        SiteSpec { position: BlockPos::new(0, 0, 0), source, source_strength: 0 }
    }

    #[test]
    fn selection_only_accepts_always_guarded_component_outputs() {
        let sites = [source_site(SiteSource::Component(NodeId::new(0)))];
        let guards = [Guard::Always, Guard::Not(GuardId::new(0))];
        assert_eq!(supported_attenuations(&[link(0, PowerPort::Output, 0), link(0, PowerPort::Stored, 3)], &sites, &guards), Some(vec![0, 3]));
        assert!(supported_attenuations(&[link(1, PowerPort::Output, 0)], &sites, &guards).is_none());
        assert!(supported_attenuations(&[link(0, PowerPort::Weak(mchprs_blocks::BlockFace::Top), 0)], &sites, &guards).is_none());
        assert!(supported_attenuations(
            &[link(0, PowerPort::Output, 0)],
            &[source_site(SiteSource::Constant(ProfileId::new(0)))],
            &guards
        )
        .is_none());
        assert!(supported_attenuations(&vec![link(0, PowerPort::Output, 0); MAX_NATIVE_LINKS + 1], &sites, &guards).is_none());
    }

    #[test]
    fn generated_c_has_per_target_saturating_attenuation_and_max() {
        let source = generate_c(&[QueryPlan { id: 7, attenuations: vec![0, 2, 15] }]);
        assert!(source.contains("case 7: return query_0(p)"));
        assert!(source.contains("p[1] > 2"));
        assert!(source.contains("p[2] > 15"));
        assert!(source.contains("if (v > m) m = v"));
    }
}
