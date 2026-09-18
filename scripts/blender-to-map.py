"""Export convex Blender meshes to Valve 220 .map and a self-contained WAD3.

Run in Blender: blender --background scene.blend --python scripts/blender-to-map.py -- --out out/scene.map
Objects use bsp_role=world|detail|entity|ignore; point entities use bsp_classname.
One Blender unit is one metre (32 GoldSrc units). Materials supply a base color
or one image texture; unsupported shader graphs and non-convex solids fail.
"""
import argparse
from collections import Counter
import json
import math
from pathlib import Path
import struct
import sys

import bpy
from mathutils import Vector


def quote(value):
    value = str(value)
    if any(c in value for c in ['"', '\n', '\r', '\x00']):
        raise ValueError(f"Invalid entity value: {value!r}")
    return '"' + value.replace('\\', '/') + '"'


def texture_pixels(material, size=64):
    image_nodes = [n for n in material.node_tree.nodes if n.type == 'TEX_IMAGE'] if material.use_nodes else []
    if len(image_nodes) > 1:
        raise ValueError(f"{material.name}: bake multiple image textures before export")
    if image_nodes:
        image = image_nodes[0].image
        if image is None:
            raise ValueError(f"{material.name}: image pixels are missing")
        width, height = image.size
        pixels = list(image.pixels)
        if width <= 0 or height <= 0 or len(pixels) != width * height * 4:
            raise ValueError(f"{material.name}: image pixels are missing")
        return [tuple(round(max(0, min(1, pixels[((height - 1 - y * height // size) * width + x * width // size) * 4 + k])) * 255)
                      for k in range(4)) for y in range(size) for x in range(size)]
    color = material.diffuse_color
    if material.use_nodes:
        shaders = [n for n in material.node_tree.nodes if n.type == 'BSDF_PRINCIPLED']
        if len(shaders) != 1 or shaders[0].inputs['Base Color'].is_linked:
            raise ValueError(f"{material.name}: bake the material to an image texture")
        color = shaders[0].inputs['Base Color'].default_value
    rgba = tuple(round(max(0, min(1, c)) * 255) for c in color)
    return [rgba] * (size * size)


def miptex(name, pixels, size=64):
    masked = name.startswith('{')
    colors = Counter(rgb[:3] for rgb in pixels if not masked or rgb[3] >= 128)
    palette = [c for c, _ in colors.most_common(255 if masked else 256)] or [(240, 240, 240)]
    lookup = {}
    def index(pixel):
        if masked and pixel[3] < 128:
            return 255
        color = pixel[:3]
        if color not in lookup:
            lookup[color] = min(range(len(palette)), key=lambda i: sum((palette[i][k] - color[k]) ** 2 for k in range(3)))
        return lookup[color]
    levels = []
    for level in range(4):
        step, width = 1 << level, size >> level
        reduced = []
        for y in range(width):
            for x in range(width):
                samples = [pixels[(y * step + dy) * size + x * step + dx] for dy in range(step) for dx in range(step)]
                reduced.append(tuple(round(sum(p[k] for p in samples) / len(samples)) for k in range(4)))
        levels.append(bytes(index(pixel) for pixel in reduced))
    offsets, offset = [], 40
    for level in levels:
        offsets.append(offset)
        offset += len(level)
    padded = palette + [(0, 0, 0)] * (256 - len(palette))
    if masked:
        padded[255] = (0, 0, 255)
    return struct.pack('<16s6I', name.encode('ascii'), size, size, *offsets) + b''.join(levels) + struct.pack('<H', 256) + bytes(c for rgb in padded for c in rgb)


def write_wad(path, materials):
    data = bytearray(b'WAD3' + bytes(8))
    entries = []
    for name, material in sorted(materials.items()):
        block = miptex(name, texture_pixels(material))
        entries.append(struct.pack('<IIIBBH16s', len(data), len(block), len(block), 0x43, 0, 0, name.encode('ascii')))
        data.extend(block)
    offset = len(data)
    data.extend(b''.join(entries))
    struct.pack_into('<II', data, 4, len(entries), offset)
    path.write_bytes(data)


def brush(obj, scale, materials):
    evaluated = obj.evaluated_get(bpy.context.evaluated_depsgraph_get())
    mesh = evaluated.to_mesh()
    try:
        vertices = [evaluated.matrix_world @ v.co * scale for v in mesh.vertices]
        if not vertices or any(not math.isfinite(v) for p in vertices for v in p):
            raise ValueError(f"{obj.name}: empty or non-finite brush")
        edge_counts = Counter(tuple(sorted(edge)) for poly in mesh.polygons for edge in poly.edge_keys)
        if not edge_counts or any(n != 2 for n in edge_counts.values()):
            raise ValueError(f"{obj.name}: brushes must be closed manifold meshes")
        center = sum(vertices, Vector()) / len(vertices)
        planes, lines = set(), []
        for poly in mesh.polygons:
            face = [vertices[i] for i in poly.vertices]
            a = face[0]
            normal = None
            for i in range(1, len(face) - 1):
                n = (face[i] - a).cross(face[i + 1] - a)
                if n.length > 1e-5:
                    b, c, normal = face[i], face[i + 1], n.normalized()
                    break
            if normal is None:
                raise ValueError(f"{obj.name}: degenerate face")
            if normal.dot(a - center) < 0:
                b, c, normal = c, b, -normal
            distance = normal.dot(a)
            if any(abs(normal.dot(p) - distance) > .025 for p in face):
                raise ValueError(f"{obj.name}: non-planar face; split the mesh into convex brushes")
            if any(normal.dot(p) > distance + .025 for p in vertices):
                raise ValueError(f"{obj.name}: non-convex brush; split the mesh into convex pieces")
            key = tuple(round(v, 5) for v in normal) + (round(distance, 3),)
            if key in planes:
                continue
            planes.add(key)
            if poly.material_index >= len(mesh.materials) or mesh.materials[poly.material_index] is None:
                raise ValueError(f"{obj.name}: every face needs a material")
            material = mesh.materials[poly.material_index]
            name = material.get('bsp_texture', material.name)
            if not name.isascii() or len(name) > 15 or not name or any(c.isspace() for c in name):
                raise ValueError(f"{material.name}: BSP texture names need 1-15 ASCII characters without spaces")
            if name in materials and materials[name] != material:
                raise ValueError(f"Duplicate texture name: {name}")
            materials[name] = material
            # Valve plane winding is opposite Blender's outward face winding.
            points = ' '.join('( ' + ' '.join(f'{x:.5f}' for x in p) + ' )' for p in [a, c, b])
            # World-aligned Valve 220 axes keep adjacent brush materials aligned.
            axis = max(range(3), key=lambda i: abs(normal[i]))
            u, v = (Vector((1, 0, 0)), Vector((0, -1, 0))) if axis == 2 else ((Vector((0, 1, 0)), Vector((0, 0, -1))) if axis == 0 else (Vector((1, 0, 0)), Vector((0, 0, -1))))
            texscale = float(material.get('bsp_scale', .5))
            if texscale <= 0 or not math.isfinite(texscale):
                raise ValueError(f'{material.name}: invalid texture scale')
            scales, shifts = [texscale, texscale], [0, 0]
            if obj.get('bsp_fit', False):
                for i, axis_vector in enumerate([u, v]):
                    values = [axis_vector.dot(p) for p in face]
                    scales[i] = (max(values) - min(values)) / 64
                    if scales[i] <= 1e-6:
                        raise ValueError(f'{obj.name}: cannot fit this face projection')
                    shifts[i] = -min(values) / scales[i]
            uv = ' '.join('[ ' + ' '.join(str(x) for x in p) + f' {shifts[i]} ]' for i, p in enumerate([u, v]))
            lines.append(f'{points} {name} {uv} 0 {scales[0]} {scales[1]}')
        if not 4 <= len(lines) <= 128:
            raise ValueError(f'{obj.name}: a brush needs 4-128 convex planes')
        return '{\n' + '\n'.join(lines) + '\n}'
    finally:
        evaluated.to_mesh_clear()


def export_scene(output, scale=32):
    output = Path(output).resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    materials, world, entities = {}, [], []
    count = 0
    for obj in sorted(bpy.context.scene.objects, key=lambda o: o.name):
        role = obj.get('bsp_role', 'world' if obj.type == 'MESH' else 'ignore')
        if role == 'ignore':
            continue
        if role == 'entity':
            props = {key[4:]: obj[key] for key in obj.keys() if key.startswith('bsp_') and key not in ['bsp_role']}
            props['origin'] = ' '.join(f'{v:.4f}' for v in obj.matrix_world.translation * scale)
            if not props.get('classname'):
                raise ValueError(f'{obj.name}: entity needs bsp_classname')
            entities.append('{\n' + '\n'.join(quote(k) + ' ' + quote(v) for k, v in props.items()) + '\n}')
        elif role in ['world', 'detail'] and obj.type == 'MESH':
            text = brush(obj, scale, materials)
            if role == 'detail':
                entities.append('{\n"classname" "func_detail"\n"zhlt_detaillevel" "1"\n' + text + '\n}')
            else:
                world.append(text)
            count += 1
        else:
            raise ValueError(f'{obj.name}: invalid BSP role {role!r}')
    wad = output.with_suffix('.wad')
    write_wad(wad, materials)
    output.write_text('{\n"classname" "worldspawn"\n"mapversion" "220"\n"wad" ' + quote(wad) + '\n"skyname" "desert"\n' + '\n'.join(world) + '\n}\n' + '\n'.join(entities) + '\n')
    receipt = {'source': bpy.data.filepath, 'map': str(output), 'wad': str(wad), 'units_per_metre': scale, 'brushes': count, 'entities': len(entities), 'textures': sorted(materials)}
    output.with_suffix('.export.json').write_text(json.dumps(receipt, indent=2))
    print(json.dumps(receipt))


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', required=True)
    parser.add_argument('--scale', type=float, default=32)
    args = parser.parse_args(sys.argv[sys.argv.index('--') + 1:])
    if not math.isfinite(args.scale) or args.scale <= 0:
        parser.error('--scale must be positive and finite')
    export_scene(args.out, args.scale)
