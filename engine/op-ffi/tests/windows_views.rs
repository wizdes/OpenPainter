#![cfg(windows)]

use op_ffi::*;
use std::ptr;

#[test]
fn hwnd_attach_rejects_a_non_window() {
    let engine = op_engine_new(ptr::null(), None, ptr::null_mut());
    assert!(!engine.is_null());
    let mut doc = 0;
    assert_eq!(
        op_import_pixels(
            engine,
            1,
            1,
            [20u8, 40, 60, 255].as_ptr(),
            4,
            ptr::null(),
            0,
            &mut doc
        ),
        OpStatus::OP_OK,
    );
    let mut view = 0;
    let result = op_view_attach(engine, doc, ptr::dangling_mut(), &mut view);
    assert_eq!(result, OpStatus::OP_ERR_BAD_ARGUMENT);
    assert_eq!(view, 0);
    op_engine_free(engine);
}

#[test]
fn panel_and_overlay_reject_bad_arguments_without_a_surface() {
    let engine = op_engine_new(ptr::null(), None, ptr::null_mut());
    assert!(!engine.is_null());
    let mut doc = 0;
    assert_eq!(
        op_doc_new(engine, 1600, 1000, 96.0, 0xffffffff, &mut doc),
        OpStatus::OP_OK
    );
    let mut view = 0;
    assert_eq!(
        op_view_attach_panel(engine, doc, ptr::null_mut(), &mut view),
        OpStatus::OP_ERR_BAD_ARGUMENT
    );
    assert_eq!(
        op_view_attach_panel(engine, doc + 1, ptr::dangling_mut(), &mut view),
        OpStatus::OP_ERR_NO_SUCH_DOC
    );
    assert_eq!(
        op_view_attach_panel(engine, doc, ptr::dangling_mut(), ptr::null_mut()),
        OpStatus::OP_ERR_BAD_ARGUMENT
    );
    assert_eq!(
        op_view_set_overlay(engine, 99, ptr::null(), 0),
        OpStatus::OP_ERR_NO_SUCH_DOC
    );
    op_engine_free(engine);
}

#[test]
fn overlay_items_validate_document_points() {
    let path = op_gpu::overlay::Item::path(
        vec![
            op_gpu::overlay::Point::new(10.0, 20.0),
            op_gpu::overlay::Point::new(50.0, 20.0),
        ],
        false,
    );
    assert!(op_gpu::overlay::Overlay::new(vec![path]).is_ok());
    assert!(
        op_gpu::overlay::Overlay::new(vec![op_gpu::overlay::Item::handle(
            op_gpu::overlay::Point::new(f32::NAN, 1.0)
        )])
        .is_err()
    );
}

#[test]
fn overlay_rejects_each_bad_argument_before_view_lookup() {
    let engine = op_engine_new(ptr::null(), None, ptr::null_mut());
    assert_eq!(
        op_view_set_overlay(engine, 99, ptr::null(), 1),
        OpStatus::OP_ERR_BAD_ARGUMENT
    );
    let bad_kind = OpOverlayItem {
        kind: 999,
        ..Default::default()
    };
    assert_eq!(
        op_view_set_overlay(engine, 99, &bad_kind, 1),
        OpStatus::OP_ERR_BAD_ARGUMENT
    );
    let bad_point = OpOverlayItem {
        kind: 1,
        point_count: 2,
        points: ptr::null(),
        ..Default::default()
    };
    assert_eq!(
        op_view_set_overlay(engine, 99, &bad_point, 1),
        OpStatus::OP_ERR_BAD_ARGUMENT
    );
    let points = [
        OpOverlayPoint { x: 0.0, y: 0.0 },
        OpOverlayPoint {
            x: f64::NAN,
            y: 1.0,
        },
    ];
    let nan_point = OpOverlayItem {
        kind: 1,
        points: points.as_ptr(),
        point_count: 2,
        ..Default::default()
    };
    assert_eq!(
        op_view_set_overlay(engine, 99, &nan_point, 1),
        OpStatus::OP_ERR_BAD_ARGUMENT
    );
    let bad_flags = OpOverlayItem {
        kind: 2,
        flags: 1,
        width: 10.0,
        height: 10.0,
        ..Default::default()
    };
    assert_eq!(
        op_view_set_overlay(engine, 99, &bad_flags, 1),
        OpStatus::OP_ERR_BAD_ARGUMENT
    );
    let negative = OpOverlayItem {
        kind: 2,
        width: -1.0,
        height: 10.0,
        ..Default::default()
    };
    assert_eq!(
        op_view_set_overlay(engine, 99, &negative, 1),
        OpStatus::OP_ERR_BAD_ARGUMENT
    );
    let short_crop = OpOverlayItem {
        kind: 6,
        points: points.as_ptr(),
        point_count: 2,
        ..Default::default()
    };
    assert_eq!(
        op_view_set_overlay(engine, 99, &short_crop, 1),
        OpStatus::OP_ERR_BAD_ARGUMENT
    );
    let corners = [
        OpOverlayPoint { x: 0.0, y: 0.0 },
        OpOverlayPoint { x: 20.0, y: 0.0 },
        OpOverlayPoint { x: 20.0, y: 20.0 },
        OpOverlayPoint { x: 0.0, y: 20.0 },
    ];
    let crop = OpOverlayItem {
        kind: 6,
        points: corners.as_ptr(),
        point_count: 4,
        ..Default::default()
    };
    assert_eq!(
        op_view_set_overlay(engine, 99, [crop, crop].as_ptr(), 2),
        OpStatus::OP_ERR_BAD_ARGUMENT
    );
    assert_eq!(
        op_view_set_overlay_dash(engine, 99, f64::NAN),
        OpStatus::OP_ERR_BAD_ARGUMENT
    );
    assert_eq!(
        op_view_set_overlay_dash(engine, 99, f64::MAX),
        OpStatus::OP_ERR_BAD_ARGUMENT
    );
    assert_eq!(
        op_view_set_overlay_dash(engine, 99, 0.0),
        OpStatus::OP_ERR_NO_SUCH_DOC
    );
    op_engine_free(engine);
}

#[test]
fn windows_overlay_shader_compiles_without_a_surface() {
    let (_instance, adapter) = op_gpu::windows_adapter().expect("DX12 adapter");
    let (device, _queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("overlay shader test"),
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .expect("DX12 device");
    device.push_error_scope(wgpu::ErrorFilter::Validation);
    let mut renderer = op_gpu::presenter::ViewRenderer::new(&device, op_gpu::win32::FORMAT);
    renderer.set_overlay(
        &device,
        Some(std::sync::Arc::new(
            op_gpu::overlay::Overlay::new(vec![op_gpu::overlay::Item::handle(
                op_gpu::overlay::Point::new(10.0, 10.0),
            )])
            .unwrap(),
        )),
        0.0,
        1.0,
        32,
        32,
    );
    let _ = device.poll(wgpu::PollType::Poll);
    assert!(pollster::block_on(device.pop_error_scope()).is_none());
}
