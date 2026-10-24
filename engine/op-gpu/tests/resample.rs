use op_gpu::GpuContext;
use op_ops::image::{Resampling, legacy};

#[test]
fn nearest_gpu_matches_cpu_with_hidden_color_and_odd_sizes() {
    let mut gpu = GpuContext::new().expect("GPU adapter");
    let source: Vec<u8> = (0..35)
        .flat_map(|i| [i as u8, 27, 91, if i % 3 == 0 { 0 } else { 128 }])
        .collect();
    let before = gpu.dispatch_count();
    let got = gpu.resample_nearest(7, 5, 3, 2, &source).unwrap();
    let want = legacy::resize(&source, 7, 5, 3, 2, Resampling::Nearest);
    assert_eq!(got, want);
    assert_eq!(gpu.dispatch_count() - before, 1);
    println!(
        "adapter={} backend={:?} dispatches=1",
        gpu.info().name,
        gpu.info().backend
    );
}
#[test]
fn bicubic_gpu_matches_cpu_with_partial_alpha() {
    let mut gpu = GpuContext::new().expect("GPU adapter");
    let source: Vec<u8> = (0..35)
        .flat_map(|i| [(i * 7) as u8, 27, 91, if i % 3 == 0 { 0 } else { 128 }])
        .collect();
    let before = gpu.dispatch_count();
    let got = gpu.resample_bicubic(7, 5, 11, 9, &source).unwrap();
    let want = legacy::resize(&source, 7, 5, 11, 9, Resampling::Bicubic);
    let worst = got
        .iter()
        .zip(&want)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap();
    assert!(worst <= 1, "worst difference {worst}");
    assert_eq!(gpu.dispatch_count() - before, 1);
    println!(
        "adapter={} backend={:?} bicubic worst={} dispatches=1",
        gpu.info().name,
        gpu.info().backend,
        worst
    );
}
#[test]
fn bilinear_gpu_matches_cpu_with_partial_alpha() {
    let mut gpu = GpuContext::new().expect("GPU adapter");
    let source: Vec<u8> = (0..35)
        .flat_map(|i| [(i * 7) as u8, 27, 91, if i % 3 == 0 { 0 } else { 128 }])
        .collect();
    let before = gpu.dispatch_count();
    let got = gpu.resample_bilinear(7, 5, 11, 9, &source).unwrap();
    let want = legacy::resize(&source, 7, 5, 11, 9, Resampling::Bilinear);
    let worst = got
        .iter()
        .zip(&want)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap();
    assert!(worst <= 1, "worst difference {worst}");
    assert_eq!(gpu.dispatch_count() - before, 1);
    println!(
        "adapter={} backend={:?} bilinear worst={} dispatches=1",
        gpu.info().name,
        gpu.info().backend,
        worst
    );
}
#[test]
fn super_sampling_gpu_matches_cpu_for_reduction() {
    let mut gpu = GpuContext::new().expect("GPU adapter");
    let source: Vec<u8> = (0..35)
        .flat_map(|i| [(i * 7) as u8, 27, 91, if i % 3 == 0 { 0 } else { 128 }])
        .collect();
    let before = gpu.dispatch_count();
    let got = gpu.resample_super_sampling(7, 5, 3, 2, &source).unwrap();
    let want = legacy::resize(&source, 7, 5, 3, 2, Resampling::SuperSampling);
    let worst = got
        .iter()
        .zip(&want)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap();
    assert!(worst <= 1, "worst difference {worst}");
    assert_eq!(gpu.dispatch_count() - before, 1);
    println!(
        "adapter={} backend={:?} super sampling worst={} dispatches=1",
        gpu.info().name,
        gpu.info().backend,
        worst
    );
}
#[test]
fn bicubic_24_mp_reports_time() {
    let mut gpu = GpuContext::new().expect("GPU adapter");
    let source = vec![128u8; 4000 * 6000 * 4];
    let start = std::time::Instant::now();
    let result = gpu
        .resample_bicubic(4000, 6000, 2000, 3000, &source)
        .unwrap();
    println!(
        "24 MP to 50% bicubic GPU: {:.3} s",
        start.elapsed().as_secs_f64()
    );
    assert_eq!(result.len(), 2000 * 3000 * 4);
}
