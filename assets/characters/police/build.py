"""Original OpenStrike patrol officer. Blender 5.1, no external assets.
Run: Blender --background --factory-startup --python-exit-code 1 --python assets/characters/police/build.py
Editable rig + GLB and a bounded quantized vertex animation for handhelds.
"""
import bpy, math, json, struct, hashlib
from pathlib import Path
from mathutils import Vector

OUT = Path(__file__).resolve().parent
bpy.ops.object.select_all(action='SELECT')
bpy.ops.object.delete(use_global=False)
scene = bpy.context.scene
scene.render.fps = 24
bpy.context.preferences.filepaths.save_version = 0
scene.render.engine = 'CYCLES'
scene.cycles.samples = 24
scene.view_settings.view_transform = 'Standard'
# x right, y forward, z up. Export x,z,-y (Pocket3D faces -Z).
PALETTE = {
    'navy': (0.085, .135, .22), 'seam': (.15,.22,.32),
    'vest': (.045,.07,.11), 'black': (.024,.032,.044),
    'skin': (.64,.39,.23), 'skin_light': (.76,.50,.31),
    'gold': (.84,.59,.18), 'silver': (.40,.48,.53),
    'gun': (.12,.15,.18), 'white': (.78,.81,.78),
    'red': (.58,.10,.10), 'blue': (.08,.15,.37),
    'hair': (.07,.035,.024), 'sole': (.065,.075,.085),
}
# One opaque palette atlas. No external images, transparency or normal maps.
img = bpy.data.images.new('Officer palette', width=64, height=64, alpha=False)
pixels = [0.0] * (64*64*4)
colors = list(PALETTE)
for y in range(64):
    for x in range(64):
        idx = min((y//16)*4 + x//16, len(colors)-1)
        rgb = PALETTE[colors[idx]]
        i = (y*64+x)*4
        pixels[i:i+4] = [*rgb, 1]
img.pixels.foreach_set(pixels)
img.filepath_raw = str(OUT/'palette.png'); img.file_format='PNG'; img.save(); img.pack()
mat = bpy.data.materials.new('Officer / one opaque atlas')
mat.use_nodes=True
bsdf=mat.node_tree.nodes.get('Principled BSDF')
bsdf.inputs['Roughness'].default_value=.85
tex=mat.node_tree.nodes.new('ShaderNodeTexImage'); tex.image=img; tex.interpolation='Closest'
mat.node_tree.links.new(tex.outputs['Color'],bsdf.inputs['Base Color'])
verts=[]; faces=[]; groups=[]; face_colors=[]; vcolors=[]

def meshpart(name, points, polys, color, bone):
    offset=len(verts)
    verts.extend(points); groups.extend([bone]*len(points)); vcolors.extend([color]*len(points))
    faces.extend([tuple(offset+i for i in p) for p in polys]); face_colors.extend([color]*len(polys))

def box(name, center, size, color, bone, bevel=0):
    x,y,z=center; a,b,c=[v/2 for v in size]
    points=[(x+sx*a,y+sy*b,z+sz*c) for sx,sy,sz in [(-1,-1,-1),(1,-1,-1),(1,1,-1),(-1,1,-1),(-1,-1,1),(1,-1,1),(1,1,1),(-1,1,1)]]
    meshpart(name,points,[(0,3,2,1),(4,5,6,7),(0,1,5,4),(1,2,6,5),(2,3,7,6),(3,0,4,7)],color,bone)

def rings(name, levels, color, bone, n=8):
    # levels = (center, x radius, y radius), horizontal ellipses.
    points=[]
    for (x,y,z),rx,ry in levels:
        for i in range(n):
            a=2*math.pi*i/n + math.pi/n
            points.append((x+rx*math.cos(a),y+ry*math.sin(a),z))
    polys=[tuple(reversed(range(n)))]
    for j in range(len(levels)-1):
        for i in range(n): polys.append((j*n+i,j*n+(i+1)%n,(j+1)*n+(i+1)%n,(j+1)*n+i))
    polys.append(tuple((len(levels)-1)*n+i for i in range(n)))
    meshpart(name,points,polys,color,bone)

def limb(name,a,b,r1,r2,color,bone,n=6):
    a,b=Vector(a),Vector(b); axis=(b-a).normalized()
    reference = Vector((0,0,1)) if abs(axis.y) > .9 else Vector((0,1,0))
    u=axis.cross(reference).normalized(); v=axis.cross(u)
    points=[]
    for center,r in [(a,r1),(b,r2)]:
        for i in range(n):
            t=2*math.pi*i/n; points.append(tuple(center+r*(math.cos(t)*u+math.sin(t)*v)))
    meshpart(name,points,[tuple(reversed(range(n))),tuple(n+i for i in range(n))]+[(i,(i+1)%n,(i+1)%n+n,i+n) for i in range(n)],color,bone)

bones=[('root',(0,0,0),(0,0,.2),None),('pelvis',(0,0,.9),(0,0,1.05),'root'),('spine',(0,0,1.05),(0,0,1.43),'pelvis'),('head',(0,0,1.49),(0,0,1.76),'spine')]
for side,s in [('L',-1),('R',1)]:
    hip=(s*.115,0,.92); knee=(s*.13,.015,.51); ankle=(s*.135,0,.13)
    shoulder=(s*.235,0,1.46); elbow=(s*.265,.17,1.22)
    hand=(.095,.355,1.275) if side=='R' else (.075,.60,1.292)
    bones += [(f'thigh.{side}',hip,knee,'pelvis'),(f'shin.{side}',knee,ankle,f'thigh.{side}'),(f'foot.{side}',ankle,(s*.135,.20,.085),f'shin.{side}'),(f'upper_arm.{side}',shoulder,elbow,'spine'),(f'forearm.{side}',elbow,hand,f'upper_arm.{side}'),(f'hand.{side}',hand,(hand[0],hand[1]+.08,hand[2]),f'forearm.{side}')]
bones += [('weapon',(.095,.355,1.275),(.095,.80,1.275),'hand.R')]
for side,s in [('L',-1),('R',1)]:
    rings('trousers upper', [((s*.115,0,.94),.105,.10),((s*.13,.008,.70),.096,.097),((s*.13,.015,.51),.082,.078)],'navy',f'thigh.{side}')
    rings('trousers lower', [((s*.13,.015,.51),.083,.079),((s*.135,0,.30),.074,.073),((s*.135,0,.14),.07,.067)],'navy',f'shin.{side}')
    box('knee seam',(s*.13,.089,.52),(.135,.012,.035),'seam',f'thigh.{side}')
    rings('boot', [((s*.135,.035,.035),.083,.155),((s*.135,.035,.095),.083,.155),((s*.135,-.01,.195),.071,.075)],'black',f'foot.{side}')
    box('sole',(s*.135,.042,.028),(.157,.30,.035),'sole',f'foot.{side}')
    shoulder=(s*.235,0,1.46); elbow=(s*.265,.17,1.22)
    hand=(.095,.355,1.275) if side=='R' else (.075,.60,1.292)
    mid=Vector(shoulder).lerp(Vector(elbow),.7)
    limb('short sleeve',shoulder,mid,.103,.09,'navy',f'upper_arm.{side}',6)
    limb('upper arm',mid,elbow,.071,.069,'skin',f'upper_arm.{side}')
    limb('forearm',elbow,hand,.068,.044,'skin_light',f'forearm.{side}')
    box('wrist cuff',hand,(.092,.085,.079),'black',f'hand.{side}')
    box('gloved hand',(hand[0],hand[1]+.048,hand[2]),(.082,.12,.087),'vest',f'hand.{side}')
    # Department shoulder shield, gold border with blue inset.
    box('shoulder patch',(s*.323,.026,1.421),(.012,.105,.096),'gold',f'upper_arm.{side}')
    box('patch field',(s*.331,.027,1.421),(.009,.082,.075),'blue',f'upper_arm.{side}')

rings('hips',[((0,0,.86),.213,.117),((0,0,1.02),.205,.115)],'navy','pelvis')
rings('shirt',[((0,0,1.00),.197,.11),((0,0,1.24),.239,.135),((0,0,1.44),.266,.125),((0,0,1.50),.17,.095)],'navy','spine')
rings('vest',[((0,.009,1.06),.207,.128),((0,.009,1.38),.252,.148),((0,.009,1.46),.196,.124)],'vest','spine')
# Collar, closure, flap pockets and hardware remain visible at 480x272.
box('shirt closure',(0,.145,1.255),(.022,.011,.315),'seam','spine')
for s in [-1,1]:
    box('collar',(s*.058,.086,1.495),(.10,.13,.038),'seam','spine')
    box('vest pocket',(s*.125,.153,1.26),(.135,.032,.135),'navy','spine')
    box('pocket flap',(s*.125,.174,1.315),(.135,.012,.034),'seam','spine')
box('name plate',(-.127,.166,1.405),(.105,.014,.026),'gold','spine')
# Shield with a pointed base (front is +Y).
meshpart('badge',[(.10,.172,1.443),(.145,.172,1.431),(.14,.172,1.384),(.10,.172,1.362),(.06,.172,1.384),(.055,.172,1.431)],[(0,1,2,3,4,5)],'gold','spine')
box('badge center',(.10,.174,1.409),(.038,.008,.024),'silver','spine')
box('body camera',(0,.161,1.401),(.043,.038,.061),'black','spine')
box('camera lens',(0,.183,1.412),(.026,.01,.024),'silver','spine')
rings('duty belt',[((0,0,1.011),.216,.132),((0,0,1.076),.216,.132)],'black','pelvis')
box('buckle',(0,.135,1.041),(.087,.022,.047),'silver','pelvis')
for x,y in [(-.167,.10),(-.23,0),(.17,.105)]:
    box('belt pouch',(x,y,1.007),(.081,.068,.13),'vest','pelvis')
box('holster',(.252,-.015,.92),(.068,.114,.22),'black','pelvis')
box('holstered grip',(.26,.02,1.04),(.058,.092,.075),'gun','pelvis')
box('radio',(-.225,-.07,1.08),(.077,.062,.16),'gun','pelvis')
limb('antenna',(-.224,-.07,1.14),(-.224,-.07,1.30),.009,.007,'black','pelvis',4)
# Anatomical head: jaw, cheekbones, brow and skull, ears and projecting nose.
limb('neck',(0,0,1.46),(0,0,1.565),.073,.074,'skin','head',8)
rings('head',[((0,.026,1.548),.072,.07),((0,.008,1.58),.099,.084),((0,0,1.67),.113,.096),((0,-.007,1.75),.103,.09),((0,-.011,1.785),.073,.069)],'skin_light','head',8)
for s in [-1,1]:
    box('ear',(s*.113,0,1.67),(.026,.042,.065),'skin','head')
    box('eyebrow',(s*.046,.094,1.701),(.065,.012,.012),'hair','head')
    box('eye white',(s*.043,.098,1.680),(.041,.009,.014),'white','head')
    box('pupil',(s*.043,.104,1.680),(.015,.006,.016),'black','head')
meshpart('nose',[(-.019,.091,1.684),(.019,.091,1.684),(-.024,.096,1.636),(.024,.096,1.636),(0,.132,1.644)],[(0,1,4),(1,3,4),(3,2,4),(2,0,4),(0,2,3,1)],'skin','head')
box('mouth',(0,.094,1.605),(.055,.008,.011),'skin','head')
rings('cap band',[((0,-.008,1.746),.12,.109),((0,-.008,1.778),.12,.109)],'black','head',8)
rings('cap crown',[((0,-.01,1.774),.135,.122),((0,-.024,1.833),.116,.098),((0,-.026,1.846),.077,.065)],'navy','head',8)
# Curved peak: flattened ellipse in front of the cap.
rings('cap peak',[((0,.115,1.759),.124,.098),((0,.115,1.773),.124,.098)],'black','head',8)
box('cap badge',(0,.109,1.802),(.04,.014,.042),'gold','head')
# Service carbine in the shoulder pocket, trigger hand and support hand.
box('receiver',(.095,.413,1.322),(.065,.27,.083),'gun','weapon')
box('upper receiver',(.095,.435,1.371),(.062,.235,.027),'black','weapon')
box('stock',(.095,.212,1.312),(.072,.18,.097),'black','weapon')
box('stock butt',(.095,.116,1.302),(.077,.028,.126),'sole','weapon')
box('handguard',(.095,.645,1.337),(.075,.21,.073),'black','weapon')
limb('barrel',(.095,.746,1.344),(.095,.963,1.344),.015,.013,'gun','weapon',6)
box('front sight',(.095,.844,1.379),(.025,.025,.056),'gun','weapon')
box('rear sight',(.095,.372,1.398),(.027,.03,.045),'black','weapon')
box('magazine',(.095,.451,1.217),(.057,.077,.157),'gun','weapon')
box('mag base',(.095,.451,1.137),(.063,.084,.017),'black','weapon')
box('pistol grip',(.095,.332,1.247),(.047,.061,.104),'black','weapon')
# Mesh + actual weighted skeleton.
mesh=bpy.data.meshes.new('Officer mesh'); mesh.from_pydata(verts,[],faces); mesh.update()
obj=bpy.data.objects.new('Patrol Officer',mesh); scene.collection.objects.link(obj); mesh.materials.append(mat)
uv=mesh.uv_layers.new(name='Palette')
for poly,c in zip(mesh.polygons,face_colors):
    i=colors.index(c); p=((i%4+.5)/4,(i//4+.5)/4)
    for loop in poly.loop_indices: uv.data[loop].uv=p
arm=bpy.data.armatures.new('Officer rig'); rig=bpy.data.objects.new('Officer rig',arm); scene.collection.objects.link(rig)
bpy.context.view_layer.objects.active=rig; rig.select_set(True); bpy.ops.object.mode_set(mode='EDIT')
for name,head,tail,parent in bones:
    bone=arm.edit_bones.new(name); bone.head=head; bone.tail=tail
    if parent: bone.parent=arm.edit_bones[parent]
bpy.ops.object.mode_set(mode='OBJECT'); rig.show_in_front=True
for name,_,_,_ in bones:
    group=obj.vertex_groups.new(name=name)
    indices=[i for i,g in enumerate(groups) if g==name]
    if indices: group.add(indices,1.0,'REPLACE')
obj.parent=rig; modifier=obj.modifiers.new('Officer skin','ARMATURE'); modifier.object=rig
# Procedural authoring writes editable per-bone keyframes. Export/bake uses
# Blender's evaluated armature, so desktop and handhelds share the same poses.
CLIPS=[('Idle',2.0,True),('Walk',1.0,True),('Run',.667,True),('Fire',.333,False),('Reload',2.0,False),('Hit',.333,False),('Death',.833,False)]

def pose(clip,t):
    for b in rig.pose.bones: b.rotation_mode='XYZ'; b.rotation_euler=(0,0,0); b.location=(0,0,0)
    def rot(n,x=0,y=0,z=0): rig.pose.bones[n].rotation_euler=tuple(math.radians(a) for a in (x,y,z))
    phase=t*2*math.pi
    if clip in ('Walk','Run'):
        phase=t*2*math.pi/(1 if clip=='Walk' else .667)
        amp=23 if clip=='Walk' else 35
        for side,offset in [('L',0),('R',math.pi)]:
            p=phase+offset
            rot('thigh.'+side,amp*math.sin(p))
            rot('shin.'+side,-max(0,math.cos(p))*amp*1.35)
            rot('foot.'+side,-amp*math.sin(p)*.35)
        rig.pose.bones['root'].location.y=.010*(1-math.cos(phase*2))
        rot('pelvis',0,2*math.sin(phase),0)
        rot('spine',-3 if clip=='Walk' else -6,-2*math.sin(phase),0)
    elif clip=='Idle':
        rot('spine',math.sin(phase/2)*.65); rot('head',0,math.sin(phase/2)*1.3,0)
    elif clip=='Fire':
        kick=max(0,1-t/.25)*math.sin(min(1,t/.045)*math.pi/2)
        rot('spine',-4*kick); rot('head',3*kick)
    elif clip=='Reload':
        a=math.sin(min(1,t/2)*math.pi)
        rot('upper_arm.L',-32*a,0,-28*a); rot('forearm.L',40*a,0,0)
        rot('spine',0,8*a); rot('head',12*a,-12*a)
        rot('upper_arm.R',-8*a)
    elif clip=='Hit':
        a=math.sin(min(1,t/.333)*math.pi)
        rot('spine',-9*a,5*a); rot('head',6*a)
    elif clip=='Death':
        a=min(1,t/.833); a=1-(1-a)**2
        # Native death fall owns world orientation; this clip relaxes limbs.
        rot('upper_arm.L',-42*a,0,-30*a); rot('upper_arm.R',-35*a,0,25*a)
        rot('head',-18*a); rot('shin.L',-18*a); rot('thigh.R',12*a)

rig.animation_data_create()
for name,duration,loop in CLIPS:
    action=bpy.data.actions.new(name); rig.animation_data.action=action
    frames=round(duration*24)
    for f in range(frames+1):
        pose(name,f/24)
        if name in ('Walk','Run'):
            bpy.context.view_layer.update()
            sole = min((rig.pose.bones[g].matrix @ arm.bones[g].matrix_local.inverted() @ Vector(v)).z for v,g in zip(verts,groups) if g.startswith('foot.'))
            rig.pose.bones['root'].location.y += .01 - sole
        for bone in rig.pose.bones:
            bone.keyframe_insert('rotation_euler',frame=f+1,group=bone.name)
            if bone.name=='root': bone.keyframe_insert('location',frame=f+1,group=bone.name)
    action.use_fake_user=True
rig.animation_data.action=bpy.data.actions['Idle']; scene.frame_set(1)
# Self-contained GLB includes all actions and the one atlas.
bpy.ops.object.select_all(action='DESELECT'); obj.select_set(True); rig.select_set(True); bpy.context.view_layer.objects.active=rig
bpy.ops.export_scene.gltf(filepath=str(OUT/'officer.glb'),export_format='GLB',use_selection=True,export_animations=True,export_animation_mode='ACTIONS',export_skins=True,export_force_sampling=True,export_frame_range=False,export_yup=True)
# OPCH v1: header, 7 clip records, per-vertex ABGR, u16 triangles, i16 xyz frames.
# Data is indexed; only unique vertices are interpolated once per visible actor.
mesh.calc_loop_triangles(); indices=[i for tri in mesh.loop_triangles for i in tri.vertices]
frame_data=bytearray(); records=[]; frames_total=0
for name,duration,loop in CLIPS:
    action=bpy.data.actions[name]; rig.animation_data.action=action
    hz = 6 if name == 'Idle' else 24 if name in ('Run','Fire') else 12
    count=max(2,round(duration*hz)+1); records.append((frames_total,count,duration,1 if loop else 0))
    for k in range(count):
        f=1+duration*24*k/(count-1); scene.frame_set(math.floor(f),subframe=f%1)
        evaluated=obj.evaluated_get(bpy.context.evaluated_depsgraph_get()); em=evaluated.to_mesh()
        for v in em.vertices:
            p=v.co; values=(p.x*70/1.846,p.z*70/1.846,-p.y*70/1.846)
            frame_data.extend(struct.pack('<hhh',*(round(a*256) for a in values)))
        evaluated.to_mesh_clear()
    frames_total+=count
blob=bytearray(struct.pack('<4s5I',b'OPCH',1,len(verts),len(indices),len(CLIPS),frames_total))
for start,count,duration,loop in records: blob.extend(struct.pack('<IIfI',start,count,duration,loop))
light = Vector((-.4,.7,.6)).normalized()
for i,c in enumerate(vcolors):
    rgb=PALETTE[c]
    # The atlas stores sRGB values. Bake soft directional contrast without
    # applying a second sRGB conversion or running lighting on the PSP CPU.
    shade = .76 + .40 * max(0, mesh.vertices[i].normal.dot(light))
    rgb8=[min(255,round(255*a*shade)) for a in rgb]
    blob.extend(bytes([*rgb8,255]))
blob.extend(struct.pack('<'+'H'*len(indices),*indices)); blob.extend(frame_data)
(OUT/'officer.opch').write_bytes(blob)
assert len(indices)//3<=1400, len(indices)//3
assert len(blob)<=512*1024,len(blob)
receipt={'generator':'Blender '+bpy.app.version_string,'triangles':len(indices)//3,'vertices':len(verts),'bones':len(bones),'materials':1,'texture':[64,64],'clips':[{'name':c[0],'duration':c[1],'frames':r[1],'looping':c[2]} for c,r in zip(CLIPS,records)],'baked_frames':frames_total,'opch_bytes':len(blob),'glb_bytes':(OUT/'officer.glb').stat().st_size,'opch_sha256':hashlib.sha256(blob).hexdigest(),'budgets':{'triangles':1400,'animation_bytes':524288,'per_actor_draws':1,'runtime_sample_hz':60,'bake_hz_default':12,'bake_hz_idle':6,'bake_hz_fast':24},'origin':'Original geometry and animation authored for OpenStrike; repository MIT license.'}
# Compare quantized/interpolated handheld frames with Blender's evaluated
# skin at the actual 60 Hz presentation cadence, including between bake keys.
qa = {}
for (name,duration,loop),(start,count,_,_) in zip(CLIPS,records):
    rig.animation_data.action=bpy.data.actions[name]
    max_error=0.0; sole_min=100.0; sole_max=-100.0
    for sample in range(math.ceil(duration*60)+1):
        t=min(duration,sample/60); f=1+t*24
        scene.frame_set(math.floor(f),subframe=f%1)
        evaluated=obj.evaluated_get(bpy.context.evaluated_depsgraph_get()); em=evaluated.to_mesh()
        key=t/duration*(count-1); a=min(count-1,int(key)); b=min(count-1,a+1); mix=key-a
        sole=100.0
        for i,v in enumerate(em.vertices):
            va=struct.unpack_from('<hhh',frame_data,((start+a)*len(verts)+i)*6)
            vb=struct.unpack_from('<hhh',frame_data,((start+b)*len(verts)+i)*6)
            baked=Vector([(x+(y-x)*mix)/256 for x,y in zip(va,vb)])
            exact=Vector((v.co.x,v.co.z,-v.co.y))*70/1.846
            max_error=max(max_error,(exact-baked).length)
            if groups[i].startswith('foot.'): sole=min(sole,baked.y)
        sole_min=min(sole_min,sole); sole_max=max(sole_max,sole)
        evaluated.to_mesh_clear()
    qa[name]={'max_position_error_units':round(max_error,4),'support_sole_min_units':round(sole_min,4),'support_sole_max_units':round(sole_max,4)}
receipt['sample_qa_60hz']=qa
assert max(v['max_position_error_units'] for v in qa.values()) < 2.0, qa
(OUT/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
# Studio views are QA assets, not shipped geometry.
rig.animation_data.action=bpy.data.actions['Idle']; scene.frame_set(1)
bpy.ops.mesh.primitive_plane_add(size=200,location=(0,0,0)); floor=bpy.context.object; floor.name='Studio floor'
floor_mat=bpy.data.materials.new('Studio floor'); floor_mat.diffuse_color=(.075,.094,.12,1); floor.data.materials.append(floor_mat)
world=bpy.data.worlds.new('Studio world'); scene.world=world; world.use_nodes=True; world.node_tree.nodes['Background'].inputs[0].default_value=(.16,.19,.24,1); world.node_tree.nodes['Background'].inputs[1].default_value=.45
for name,loc,power,size in [('Key',(3,4,5),450,4),('Fill',(-3,2,3),180,3),('Rim',(1,-3,4),600,3)]:
    data=bpy.data.lights.new(name,'AREA'); data.energy=power; data.shape='DISK'; data.size=size
    light=bpy.data.objects.new(name,data); scene.collection.objects.link(light); light.location=loc; light.rotation_euler=(Vector((0,0,1))-light.location).to_track_quat('-Z','Y').to_euler()
data=bpy.data.cameras.new('Review camera'); cam=bpy.data.objects.new('Review camera',data); scene.collection.objects.link(cam); scene.camera=cam
cam.location=(2.7,5.0,2.5); cam.rotation_euler=(Vector((0,.10,.93))-cam.location).to_track_quat('-Z','Y').to_euler(); data.type='ORTHO'; data.ortho_scale=2.4
scene.render.resolution_x=720; scene.render.resolution_y=840; scene.render.resolution_percentage=100
bpy.ops.wm.save_as_mainfile(filepath=str(OUT/'officer.blend'))
scene.render.filepath=str(OUT/'preview.png'); bpy.ops.render.render(write_still=True)
print('OFFICER RECEIPT',json.dumps(receipt))
