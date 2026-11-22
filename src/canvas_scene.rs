use crate::prelude::*;
use std::collections::HashMap;

const LAYERS: [usize; 3] = [LYR_TILE, LYR_FLOAT, LYR_UNMANAGED];

#[derive(Default)]
pub struct Projection {
    world: *mut wlr_scene,
    layers: [*mut wlr_scene_tree; 3],
    copies: HashMap<usize, Box<CopyNode>>,
}

struct CopyNode {
    node: *mut wlr_scene_node,
    source: *mut wlr_scene_node,
    visited: bool,
    source_destroy: Listener,
    copy_destroy: Listener,
}

unsafe fn destroy_source(copy: *mut CopyNode, _: *mut c_void) {
    let copy = g!()
        .canvas
        .projection
        .copies
        .remove(&((*copy).source as usize));
    if let Some(copy) = copy {
        if !copy.node.is_null() {
            wlr_scene_node_destroy(copy.node);
        }
    }
}

unsafe fn destroy_copy(copy: *mut CopyNode, _: *mut c_void) {
    (*copy).node = null_mut();
    (*copy).copy_destroy.remove();
}

pub unsafe fn init() {
    g!().canvas.projection.world = wlr_scene_create();
    for (index, layer) in LAYERS.into_iter().enumerate() {
        let original = g!().layers[layer];
        let projected = wlr_scene_tree_create(&mut (*g!().scene).tree);
        wlr_scene_node_place_above(&mut (*projected).node, &mut (*original).node);
        wlr_scene_node_reparent(
            &mut (*original).node,
            &mut (*g!().canvas.projection.world).tree,
        );
        g!().canvas.projection.layers[index] = projected;
    }
}

unsafe fn copy_node(
    source: *mut wlr_scene_node,
    parent: *mut wlr_scene_tree,
) -> *mut wlr_scene_node {
    if let Some(copy) = g!().canvas.projection.copies.get_mut(&(source as usize)) {
        if !copy.node.is_null() {
            copy.visited = true;
            if (*copy.node).parent != parent {
                wlr_scene_node_reparent(copy.node, parent);
            }
            return copy.node;
        }
    }
    let projected = if (*source).type_ == WLR_SCENE_NODE_RECT {
        let rect = wlr_scene_rect_from_node(source);
        &mut (*wlr_scene_rect_create(parent, 0, 0, (*rect).color.as_ptr())).node
    } else {
        let buffer = wlr_scene_buffer_from_node(source);
        let surface = wlr_scene_surface_try_from_buffer(buffer);
        if surface.is_null() {
            &mut (*wlr_scene_buffer_create(parent, (*buffer).buffer)).node
        } else {
            &mut (*(*wlr_scene_surface_create(parent, (*surface).surface)).buffer).node
        }
    };
    let mut copy = Box::new(CopyNode {
        node: projected,
        source,
        visited: true,
        source_destroy: Listener::none(),
        copy_destroy: Listener::none(),
    });
    let owner = &mut *copy as *mut CopyNode;
    copy.source_destroy = Listener::new(&mut (*source).events.destroy, owner, destroy_source);
    copy.copy_destroy = Listener::new(&mut (*projected).events.destroy, owner, destroy_copy);
    g!().canvas.projection.copies.insert(source as usize, copy);
    projected
}

unsafe fn project(
    source: *mut wlr_scene_node,
    parent: *mut wlr_scene_tree,
    x: f64,
    y: f64,
    enabled: bool,
    monitor: *mut Monitor,
    previous: &mut *mut wlr_scene_node,
) {
    let monitor = if !(*source).data.is_null() && LAYERS.iter().any(|layer| (*source).parent == g!().layers[*layer]) {
        let client = (*source).data as *mut Client;
        (*client).mon
    } else { monitor };
    let x = x + (*source).x as f64;
    let y = y + (*source).y as f64;
    let enabled = enabled && (*source).enabled;
    if (*source).type_ == WLR_SCENE_NODE_TREE {
        let tree = wlr_scene_tree_from_node(source);
        for child in wl_list_items!(&mut (*tree).children, wlr_scene_node, link) {
            project(child, parent, x, y, enabled, monitor, previous);
        }
        return;
    }
    let projected = copy_node(source, parent);
    let (camera, scale) = crate::workspace::camera(monitor);
    let left = ((x - camera.x) * scale).round() as i32;
    let top = ((y - camera.y) * scale).round() as i32;
    if previous.is_null() {
        if (*parent).children.next != &mut (*projected).link {
            wlr_scene_node_lower_to_bottom(projected);
        }
    } else if (*projected).link.prev != &mut (**previous).link {
        wlr_scene_node_place_above(projected, *previous);
    }
    *previous = projected;
    let rectangle = (*source).type_ == WLR_SCENE_NODE_RECT;
    let (width, height) = if rectangle {
        let rect = wlr_scene_rect_from_node(source);
        ((*rect).width, (*rect).height)
    } else {
        let buffer = wlr_scene_buffer_from_node(source);
        let (mut width, mut height) = ((*buffer).dst_width, (*buffer).dst_height);
        if width == 0 && height == 0 {
            width = (*buffer).WLR_PRIVATE.buffer_width;
            height = (*buffer).WLR_PRIVATE.buffer_height;
            if (*buffer).transform & 1 != 0 {
                std::mem::swap(&mut width, &mut height);
            }
        }
        (width, height)
    };
    let frame = wlr_box(left, top,
        (((x + width as f64 - camera.x) * scale).round() as i32 - left).max(1),
        (((y + height as f64 - camera.y) * scale).round() as i32 - top).max(1));
    let mut clipped = frame;
    let intersects = monitor.is_null() || wlr_box_intersection(&mut clipped, &frame, &(*monitor).m);
    wlr_scene_node_set_enabled(projected, enabled && intersects);
    if !intersects { return; }
    wlr_scene_node_set_position(projected, clipped.x, clipped.y);
    if rectangle {
        let copy = wlr_scene_rect_from_node(projected);
        wlr_scene_rect_set_size(copy, clipped.width, clipped.height);
        wlr_scene_rect_set_color(copy, (*wlr_scene_rect_from_node(source)).color.as_ptr());
    } else {
        let buffer = wlr_scene_buffer_from_node(source);
        let copy = wlr_scene_buffer_from_node(projected);
        wlr_scene_buffer_set_dest_size(copy, clipped.width, clipped.height);
        wlr_scene_buffer_set_source_box(copy, &crop(buffer, frame, clipped));
        wlr_scene_buffer_set_transform(copy, (*buffer).transform);
        wlr_scene_buffer_set_opacity(copy, (*buffer).opacity);
        wlr_scene_buffer_set_filter_mode(copy, WLR_SCALE_FILTER_BILINEAR);
        wlr_scene_buffer_set_transfer_function(copy, (*buffer).transfer_function);
        wlr_scene_buffer_set_primaries(copy, (*buffer).primaries);
        wlr_scene_buffer_set_color_encoding(copy, (*buffer).color_encoding);
        wlr_scene_buffer_set_color_range(copy, (*buffer).color_range);
        if wlr_scene_surface_try_from_buffer(buffer).is_null() {
            if (*copy).buffer != (*buffer).buffer {
                wlr_scene_buffer_set_buffer_with_damage(copy, (*buffer).buffer, null());
            }
        }
    }
}

unsafe fn crop(buffer: *mut wlr_scene_buffer, frame: wlr_box, clipped: wlr_box) -> wlr_fbox {
    let mut source = (*buffer).src_box;
    if clipped.width == frame.width && clipped.height == frame.height { return source; }
    if source.width == 0.0 || source.height == 0.0 {
        source = wlr_fbox { width: (*buffer).WLR_PRIVATE.buffer_width as f64,
            height: (*buffer).WLR_PRIVATE.buffer_height as f64, ..Default::default() };
    }
    let relative = wlr_fbox { x: (clipped.x - frame.x) as f64 / frame.width as f64,
        y: (clipped.y - frame.y) as f64 / frame.height as f64,
        width: clipped.width as f64 / frame.width as f64, height: clipped.height as f64 / frame.height as f64 };
    let mut normalized = wlr_fbox::default();
    let inverse = match (*buffer).transform { 1 => 3, 3 => 1, other => other };
    wlr_fbox_transform(&mut normalized, &relative, inverse, 1.0, 1.0);
    source.x += normalized.x * source.width;
    source.y += normalized.y * source.height;
    source.width *= normalized.width;
    source.height *= normalized.height;
    source
}

pub unsafe fn sync() {
    for copy in g!().canvas.projection.copies.values_mut() {
        copy.visited = false;
    }
    for (index, layer) in LAYERS.into_iter().enumerate() {
        let mut previous = null_mut();
        project(
            &mut (*g!().layers[layer]).node,
            g!().canvas.projection.layers[index],
            0.0,
            0.0,
            true,
            null_mut(),
            &mut previous,
        );
    }
    let detached: Vec<_> = g!()
        .canvas
        .projection
        .copies
        .iter()
        .filter_map(|(source, copy)| (!copy.visited).then_some(*source))
        .collect();
    for source in detached {
        let copy = g!().canvas.projection.copies.remove(&source).unwrap();
        if !copy.node.is_null() {
            wlr_scene_node_destroy(copy.node);
        }
    }
}

pub unsafe fn finish() {
    wlr_scene_node_destroy(&mut (*g!().canvas.projection.world).tree.node);
    g!().canvas.projection.world = null_mut();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_clips_rectangles_and_rotated_buffers_to_their_output() {
        unsafe {
            let frame = wlr_box(-50, 0, 100, 100);
            let mut clipped = wlr_box::default();
            assert!(wlr_box_intersection(&mut clipped, &frame, &wlr_box(0, 0, 100, 100)));
            assert_eq!((clipped.x, clipped.width), (0, 50));
            for transform in 0..8 {
                let mut buffer = wlr_scene_buffer { src_box: wlr_fbox { x: 10.0, y: 20.0, width: 200.0, height: 100.0 }, transform, ..Default::default() };
                let source = crop(&mut buffer, frame, clipped);
                assert!(source.x >= 10.0 && source.y >= 20.0);
                assert!(source.x + source.width <= 210.0 && source.y + source.height <= 120.0);
                assert_eq!(source.width * source.height, 10000.0);
                if transform == 0 { assert_eq!((source.x, source.width), (110.0, 100.0)); }
            }
        }
    }
}
