"""Author an editable Apple Park atrium route from the WWDC24 macOS transition.
The visible bridge, two stair flights and presentation hall are reconstructed;
dimensions, back corridors, garden layout and spawn placement are authored.
Run: blender --background --factory-startup --python scripts/build-parkour.py -- --out out/parkour
"""
import argparse
import json
import math
from pathlib import Path
import random
import sys

import bpy
from mathutils import Vector

TEXTURE_ROOT = None

def material(name, rgb, pattern='stone', scale=.5):
    mat = bpy.data.materials.new(name)
    mat.use_nodes = True
    mat['bsp_scale'] = scale
    mat.diffuse_color = (*rgb, 1)
    shader = mat.node_tree.nodes.get('Principled BSDF')
    shader.inputs['Roughness'].default_value = .55
    size = 64
    pixels = []
    rng = random.Random(name)
    for y in range(size):
        for x in range(size):
            k, alpha = 1.0, 1.0
            if pattern == 'stone':
                k = .92 if x == 0 or y == 0 else 1 - rng.randrange(4) / 128
            elif pattern == 'wood':
                k = .83 + ((x * 3 + rng.randrange(4)) % 9) / 60
            elif pattern == 'grass':
                k = .68 + rng.randrange(10) / 30
            elif pattern == 'glass':
                alpha = 1 if x % 4 == 0 and y % 4 == 0 else 0
                k = .95
            elif pattern == 'display':
                bands = [(0.16, .48, .73), (.14, .66, .72), (.98, .52, .2), (.65, .2, .42), (.3, .2, .52)]
                rgb = bands[min(4, int((y + x * .28) / 17))]
            pixels.extend((*[min(1, c * k) for c in rgb], alpha))
    image = bpy.data.images.new(name + '_image', width=size, height=size, alpha=True)
    image.pixels.foreach_set(pixels)
    image.update()
    image.filepath_raw = str(TEXTURE_ROOT / (name + '.png'))
    image.file_format = 'PNG'
    image.save()
    image.pack()
    node = mat.node_tree.nodes.new('ShaderNodeTexImage')
    node.image = image
    mat.node_tree.links.new(node.outputs['Color'], shader.inputs['Base Color'])
    if pattern == 'glass':
        mat.node_tree.links.new(node.outputs['Alpha'], shader.inputs['Alpha'])
        mat.surface_render_method = 'DITHERED'
    return mat


def box(name, center, dimensions, mat, role='world', rotation=(0, 0, 0)):
    bpy.ops.mesh.primitive_cube_add(size=1, location=center, rotation=rotation)
    obj = bpy.context.object
    obj.name = name
    obj.dimensions = dimensions
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    obj.data.materials.append(mat)
    obj['bsp_role'] = role
    if mat.name == 'sky':
        obj.hide_render = True
    return obj


def beam(name, start, end, width, height, mat, role='detail'):
    delta = Vector(end) - Vector(start)
    obj = box(name, (Vector(start) + Vector(end)) / 2, (width, delta.length, height), mat, role)
    obj.rotation_euler = delta.to_track_quat('Y', 'Z').to_euler()
    return obj


def entity(name, classname, position, **props):
    obj = bpy.data.objects.new(name, None)
    bpy.context.collection.objects.link(obj)
    obj.location = position
    obj['bsp_role'] = 'entity'
    obj['bsp_classname'] = classname
    for key, value in props.items():
        obj['bsp_' + key] = str(value)
    return obj


def title_material(out):
    """Render the reference's presentation title into an original WAD image."""
    scene = bpy.data.scenes.new('macOS_title_texture')
    scene.world = bpy.data.worlds.new('title_white')
    scene.world.use_nodes = True
    scene.world.node_tree.nodes['Background'].inputs['Color'].default_value = (1, 1, 1, 1)
    scene.view_settings.view_transform = 'Standard'
    curve = bpy.data.curves.new('macOS_title', 'FONT')
    curve.body = 'macOS'; curve.align_x = 'CENTER'; curve.align_y = 'CENTER'; curve.size = 1.15
    text = bpy.data.objects.new('macOS_title', curve); scene.collection.objects.link(text)
    ink = bpy.data.materials.new('title_ink'); ink.use_nodes = True
    nodes = ink.node_tree.nodes; nodes.clear()
    emission = nodes.new('ShaderNodeEmission'); emission.inputs['Color'].default_value = (.035, .22, .44, 1)
    output = nodes.new('ShaderNodeOutputMaterial'); ink.node_tree.links.new(emission.outputs[0], output.inputs['Surface'])
    curve.materials.append(ink)
    camera_data = bpy.data.cameras.new('title_camera'); camera_data.type = 'ORTHO'; camera_data.ortho_scale = 6
    camera = bpy.data.objects.new('title_camera', camera_data); scene.collection.objects.link(camera); camera.location = (0, 0, 10); scene.camera = camera
    scene.render.engine = 'CYCLES'; scene.cycles.samples = 1
    scene.render.resolution_x = 512; scene.render.resolution_y = 256; scene.render.resolution_percentage = 100
    scene.render.filepath = str(out / 'textures/title.png'); scene.render.image_settings.file_format = 'PNG'
    bpy.ops.render.render(scene=scene.name, write_still=True)
    image = bpy.data.images.load(scene.render.filepath); image.pack()
    mat = bpy.data.materials.new('ap_title'); mat.use_nodes = True
    tex = mat.node_tree.nodes.new('ShaderNodeTexImage'); tex.image = image
    mat.node_tree.links.new(tex.outputs['Color'], mat.node_tree.nodes['Principled BSDF'].inputs['Base Color'])
    bpy.data.scenes.remove(scene)
    return mat


def make(out):
    global TEXTURE_ROOT
    bpy.ops.object.select_all(action='SELECT')
    bpy.ops.object.delete(use_global=False)
    out.mkdir(parents=True, exist_ok=True)
    TEXTURE_ROOT = out / 'textures'
    TEXTURE_ROOT.mkdir(exist_ok=True)
    stone = material('ap_stone', (.89, .875, .82), scale=1)
    white = material('ap_white', (.97, .965, .94), 'flat', scale=2)
    floor = material('ap_floor', (.78, .79, .77), scale=1)
    metal = material('ap_metal', (.48, .55, .56), 'flat', scale=2)
    wood = material('ap_wood', (.57, .40, .24), 'wood')
    grass = material('ap_lawn', (.24, .39, .14), 'grass')
    foliage = material('ap_leaf', (.20, .38, .13), 'grass')
    glass = material('{ap_glass', (.73, .88, .91), 'glass', .5)
    sky = material('sky', (.6, .79, .91), 'flat', scale=2)
    display = title_material(out)
    # A sealed envelope provides finite collision/PVS. Sky faces render as sky.
    box('sky_roof', (0, 0, 19), (58, 106, 1), sky)
    for x in [-29, 29]: box('sky_side', (x, 0, 9), (1, 106, 20), sky)
    for y in [-53, 53]: box('sky_end', (0, y, 9), (58, 1, 20), sky)
    box('landscape_base', (0, 0, -.55), (58, 106, 1), grass)
    box('atrium_floor', (0, 0, -.16), (14, 48, .32), floor)
    box('presentation_floor', (0, 29, -.16), (28, 20, .32), floor)
    box('stair_base', (9.5, 0, -.16), (5, 18, .32), floor)
    # Limestone walls with doors onto back passages. Per-floor courses retain
    # the horizontal rhythm of the reference without tiny geometry.
    for side in [-1, 1]:
        for z in [2, 6, 10]:
            for y, length in [(-18, 12), (15, 18)]:
                box('atrium_limestone', (side * 7.2, y, z), (.4, length, 4), stone)
            if side == -1:
                box('west_middle_wall', (side * 7.2, 0, z), (.4, 12, 4), stone)
        box('garden_gallery', (side * 8.6, 0, -.14), (3.2, 48, .28), floor)
        for z in [4, 8]:
            box('upper_walkway', (side * 5.45, 0, z - .18), (3.5, 48, .36), white)
            # Leave the whole cross-bridge opening clear for the standing hull.
            for begin, end in [(-24, -11.4), (-8.6, 10.6), (13.4, 24)]:
                length = end - begin
                box('glass_balustrade', (side * 3.73, (begin + end) / 2, z + .52), (.065, length, 1.04), glass, 'detail')
                box('glass_top_rail', (side * 3.73, (begin + end) / 2, z + 1.04), (.10, length, .055), metal, 'detail')
    for z in [4, 8]:
        for y in [-10, 12]:
            box('cross_bridge', (0, y, z - .18), (7.5, 2.8, .36), white)
            for edge in [-1, 1]:
                box('bridge_glass', (0, y + edge * 1.38, z + .52), (7.5, .065, 1.04), glass, 'detail')
                box('bridge_rail', (0, y + edge * 1.38, z + 1.04), (7.5, .10, .055), metal, 'detail')
    # Skylight ribs, clerestory glazing and thin side columns.
    for y in range(-24, 25, 3):
        box('skylight_rib', (0, y, 12), (15, .22, .32), white, 'detail')
        for side in [-1, 1]:
            box('gallery_column', (side * 6.85, y, 6), (.28, .3, 12), white, 'detail')
    box('skylight_glazing', (0, 0, 12.2), (14, 48, .045), glass, 'detail')
    # Stair alcove: the two opposing flights seen between bridge and hall.
    box('stair_outer_wall', (12.25, 0, 6), (.5, 18, 12), stone)
    for y in [-9, 9]: box('stair_end_wall', (9.5, y, 6), (5.5, .4, 12), stone)
    for y, z in [(-6.8, 8), (6.8, 4), (-6.8, 0)]:
        box('stair_landing', (9.5, y, z - .18), (5, 3.6, .36), floor)
    for i in range(20):
        y = -4.75 + i * .5
        z = 8 - (i + 1) * .2
        box(f'upper_stair_{i:02}', (8.15, y, z - .22), (2.3, .5, .44), white)
        y = 4.75 - i * .5
        z = 4 - (i + 1) * .2
        box(f'lower_stair_{i:02}', (10.85, y, z - .22), (2.3, .5, .44), white)
    for x in [7.02, 9.3]:
        beam('upper_solid_rail', (x, -5, 8.65), (x, 5, 4.65), .18, 1.15, white)
    for x in [9.70, 12]:
        beam('lower_solid_rail', (x, 5, 4.65), (x, -5, .65), .18, 1.15, white)
    # Presentation room is open to the atrium and garden glazing.
    for x in [-14, 14]:
        box('hall_glass_wall', (x, 29, 3), (.06, 20, 6), glass, 'detail')
        for y in [20, 24, 28, 32, 36, 39]:
            box('hall_mullion', (x, y, 3), (.2, .2, 6), metal, 'detail')
    box('hall_rear_glass', (0, 39, 3), (28, .06, 6), glass, 'detail')
    for y in [20, 24, 28, 32, 36, 40]:
        box('hall_roof_beam', (0, y, 6.1), (28, .35, .42), white, 'detail')
    box('hall_roof_glass', (0, 30, 6.35), (28, 20, .06), glass, 'detail')
    box('presentation_wall', (0, 35, 2.65), (14, .45, 5.3), white)
    board = box('keynote_display', (0, 34.74, 2.85), (9.8, .04, 3.9), display, 'detail')
    board['bsp_fit'] = True
    for face in board.data.polygons:
        for index in face.loop_indices:
            p = board.data.vertices[board.data.loops[index].vertex_index].co
            board.data.uv_layers.active.data[index].uv = (p.x / 9.8 + .5, p.z / 3.9 + .5)
    for x in [-2.55, 2.55]:
        for y in [-17, 3]:
            box('atrium_planter', (x, y, .22), (2.0, 5, .44), stone, 'detail')
            box('atrium_green', (x, y, .5), (1.9, 4.9, .2), foliage, 'detail')
            bpy.ops.mesh.primitive_cylinder_add(vertices=8, radius=.13, depth=2.6, location=(x, y, 1.7))
            trunk = bpy.context.object; trunk.name = 'atrium_tree_trunk'; trunk.data.materials.append(wood); trunk['bsp_role'] = 'detail'
            bpy.ops.mesh.primitive_ico_sphere_add(subdivisions=1, radius=1.15, location=(x, y, 3.3))
            crown = bpy.context.object; crown.name = 'atrium_tree_crown'; crown.data.materials.append(foliage); crown['bsp_role'] = 'detail'
    # Back corridors make the reconstruction a connected multiplayer space.
    # These and their cover are authored additions, outside the filmed route.
    for side in [-1, 1]:
        box('back_corridor', (side * 16, 5, -.14), (3.6, 54, .28), floor)
        for y in [-20, -8, 8, 20, 31]:
            box('garden_planter', (side * 19, y, .32), (3, 4, .64), stone, 'detail')
            box('garden_shrub', (side * 19, y, .94), (2.8, 3.8, .6), foliage, 'detail')
        for y in [-15, 5, 25]:
            box('gallery_bench', (side * 9, y, .5), (1, 2.8, .16), wood, 'detail')
            for dy in [-1, 1]: box('bench_leg', (side * 9, y + dy, .25), (.12, .12, .5), metal, 'detail')
    for x, y in [(-23, -30), (-21, 34), (22, 28), (23, -16), (-22, 6)]:
        bpy.ops.mesh.primitive_cylinder_add(vertices=8, radius=.22, depth=4, location=(x, y, 2))
        trunk = bpy.context.object; trunk.name = 'garden_tree_trunk'; trunk.data.materials.append(wood); trunk['bsp_role'] = 'detail'
        bpy.ops.mesh.primitive_ico_sphere_add(subdivisions=1, radius=2.3, location=(x, y, 4.8))
        crown = bpy.context.object; crown.name = 'garden_tree_crown'; crown.data.materials.append(foliage); crown['bsp_role'] = 'detail'
    # Empty entities are editable spawn/light markers in the .blend source.
    entity('CT_route_start', 'info_player_start', (0, -10, 9.125), angle=90)
    entity('CT_upper_gallery', 'info_player_start', (-5.4, -16, 9.125), angle=90)
    entity('T_hall', 'info_player_deathmatch', (0, 28, 1.125), angle=270)
    entity('T_garden', 'info_player_deathmatch', (-16, 12, 1.125), angle=270)
    entity('sun', 'light_environment', (0, 0, 16), angles='-60 125 0', _light='255 244 229 180', _diffuse_light='192 216 255 65')
    for y in [-18, -8, 3, 14, 26, 33]:
        entity('fill_light', 'light', (0, y, 10 if y < 20 else 5.7), _light='225 239 255 230')
    # Blender preview mirrors the authored materials and architectural framing.
    bpy.ops.object.light_add(type='SUN', rotation=(math.radians(30), math.radians(-25), math.radians(-35)))
    bpy.context.object.data.energy = 2.5
    bpy.context.object['bsp_role'] = 'ignore'
    scene = bpy.context.scene
    scene.world.color = (.35, .40, .48)
    scene.world.use_nodes = True
    background = scene.world.node_tree.nodes.get('Background')
    background.inputs['Color'].default_value = (.55, .66, .8, 1)
    background.inputs['Strength'].default_value = .6
    bpy.ops.object.camera_add(location=(0, -10, 9.9))
    camera = bpy.context.object; camera['bsp_role'] = 'ignore'
    camera.rotation_euler = (Vector((0, 9, 6.3)) - camera.location).to_track_quat('-Z', 'Y').to_euler()
    camera.data.lens = 22
    scene.camera = camera
    scene.render.engine = 'CYCLES'
    scene.cycles.samples = 24
    scene.render.resolution_x, scene.render.resolution_y, scene.render.resolution_percentage = 1280, 720, 100
    scene.render.image_settings.file_format = 'PNG'
    scene.render.filepath = str(out / 'atrium-preview.png')
    scene['reference_url'] = 'https://developer.apple.com/videos/play/wwdc2024/101/'
    scene['reference_seconds'] = '3040-3057; WWDC24 iPadOS-to-macOS transition'
    scene['reconstruction_scope'] = 'Visible bridge/stair/hall route; inferred dimensions and authored connecting galleries/gardens'
    bpy.ops.wm.save_as_mainfile(filepath=str(out / 'wwdc24-parkour.blend'))
    # Route centres, in Blender metres, follow ordinary stairs without a
    # teleport or character-specific movement rule.
    route = [(0,-10,9.125), (5.4,-10,9.125), (5.4,-7.3,9.125), (8.15,-7.3,9.125),
             (8.15,-5.2,9.125), (8.15,5.5,5.125), (8.15,6.8,5.125), (10.85,6.8,5.125),
             (10.85,5.2,5.125), (10.85,-5.5,1.125), (10.85,-6.8,1.125), (5.4,-6.8,1.125),
             (0,-6.8,1.125), (0,16,1.125), (0,28,1.125)]
    (out / 'route.json').write_text(json.dumps({'units':'Blender metres, Z up', 'waypoints':route}, indent=2))
    (out / 'route.txt').write_text('\n'.join(f'{x*32} {z*32} {-y*32}' for x,y,z in route)+'\n')
    bpy.ops.render.render(write_still=True)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(); parser.add_argument('--out', required=True)
    args = parser.parse_args(sys.argv[sys.argv.index('--') + 1:])
    make(Path(args.out).resolve())
