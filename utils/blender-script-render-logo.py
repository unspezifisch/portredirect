import bpy
import os
import random


def cleanup_scene():
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)


def setup_random_seed(seed=42):
    random.seed(seed)


def create_background_pipes():
    for i in range(20):
        # Define parameters for the outer pipe
        radius = 0.5
        depth = random.uniform(4, 6)
        x = random.uniform(-8, 8)
        y = random.uniform(13, 17)  # placed in the distance
        z = random.uniform(-20, -10)

        # Create the outer cylinder
        bpy.ops.mesh.primitive_cylinder_add(
            radius=radius, depth=depth, location=(x, y, z)
        )
        outer_pipe = bpy.context.object
        outer_pipe.name = f"DimPipe_{i}"

        # Create an inner cylinder to hollow out the pipe
        inner_radius = radius - 0.15  # slightly smaller than the outer radius
        inner_depth = depth + 0.2  # slightly taller for a clean subtraction
        bpy.ops.mesh.primitive_cylinder_add(
            radius=inner_radius, depth=inner_depth, location=(x, y, z)
        )
        inner_pipe = bpy.context.object
        inner_pipe.name = f"DimPipeInner_{i}"

        # Apply a Boolean difference modifier to subtract the inner from the outer
        bool_mod = outer_pipe.modifiers.new(name="PipeHole", type="BOOLEAN")
        bool_mod.operation = "DIFFERENCE"
        bool_mod.object = inner_pipe
        bpy.context.view_layer.objects.active = outer_pipe
        bpy.ops.object.modifier_apply(modifier=bool_mod.name)

        # Delete the inner cylinder
        bpy.data.objects.remove(inner_pipe, do_unlink=True)

        # Assign a dim, off-color material to the now-hollow outer pipe
        dim_mat = bpy.data.materials.new(name=f"DimPipeMat_{i}")
        dim_mat.use_nodes = True
        dnodes = dim_mat.node_tree.nodes
        dprincipled = dnodes.get("Principled BSDF")
        dprincipled.inputs["Base Color"].default_value = (
            0.2,
            0.2,
            0.3,
            1,
        )  # dark, off-color blue/grey
        dprincipled.inputs["Metallic"].default_value = 0.2
        dprincipled.inputs["Roughness"].default_value = 0.8
        outer_pipe.data.materials.append(dim_mat)


def create_clouds():
    for i in range(13):
        # Base position for the cloud
        base_x = random.uniform(-20, 20)
        base_y = random.uniform(10, 50)
        base_z = random.uniform(-30, -20)

        # Determine how many ellipses (parts) will form this cloud
        num_parts = random.randint(1, 3)
        cloud_parts = []

        for j in range(num_parts):
            # Random offset for each ellipse relative to the base position
            offset_x = random.uniform(-1.0, 1.0)
            offset_y = random.uniform(-1.0, 1.0)
            offset_z = random.uniform(-0.2, 0.2)  # slight variation in depth

            pos_x = base_x + offset_x
            pos_y = base_y + offset_y
            pos_z = base_z + offset_z

            # Create a filled ellipse (disk)
            bpy.ops.mesh.primitive_circle_add(
                vertices=32, radius=1, fill_type="NGON", location=(pos_x, pos_y, pos_z)
            )
            ellipse = bpy.context.object
            ellipse.name = f"Cloud_{i}_{j}"

            # Apply random scaling for the ellipse shape
            scale_x = random.uniform(2.0, 3.0)
            scale_y = random.uniform(1.0, 1.5)
            scale_z = random.uniform(0.3, 0.5)
            ellipse.scale = (scale_x, scale_y, scale_z)

            # Optionally, add a slight random rotation around Z for variation
            ellipse.rotation_euler[2] = random.uniform(0, 6.28319)

            # Create and assign a simple cloud material with a mix of Principled and Emission shaders
            cloud_mat = bpy.data.materials.new(name=f"CloudMaterial_{i}_{j}")
            cloud_mat.use_nodes = True
            c_nodes = cloud_mat.node_tree.nodes
            c_principled = c_nodes.get("Principled BSDF")
            c_principled.inputs["Base Color"].default_value = (0.9, 0.9, 0.9, 1)
            c_principled.inputs["Roughness"].default_value = 1.0

            # Create an Emission node for a subtle glow
            emission = c_nodes.new(type="ShaderNodeEmission")
            emission.inputs["Color"].default_value = (1, 1, 1, 1)
            emission.inputs["Strength"].default_value = 0.2

            # Mix the two shaders
            mix_shader = c_nodes.new(type="ShaderNodeMixShader")
            links = cloud_mat.node_tree.links
            links.new(c_principled.outputs["BSDF"], mix_shader.inputs[1])
            links.new(emission.outputs["Emission"], mix_shader.inputs[2])
            mix_shader.inputs["Fac"].default_value = 0.3

            output = c_nodes.get("Material Output")
            links.new(mix_shader.outputs["Shader"], output.inputs["Surface"])

            ellipse.data.materials.append(cloud_mat)
            cloud_parts.append(ellipse)

        # Optionally join the parts so each cloud is a single object
        if len(cloud_parts) > 1:
            bpy.context.view_layer.objects.active = cloud_parts[0]
            for obj in cloud_parts:
                obj.select_set(True)
            bpy.ops.object.join()
            cloud_parts[0].name = f"Cloud_{i}"


def create_hollow_pipe():
    # Create outer cylinder
    bpy.ops.mesh.primitive_cylinder_add(radius=1, depth=5, location=(0, 0, 0))
    outer_pipe = bpy.context.object
    outer_pipe.name = "OuterPipe"

    # Create inner cylinder (slightly smaller and taller for a clean boolean)
    bpy.ops.mesh.primitive_cylinder_add(radius=0.8, depth=5.1, location=(0, 0, 0))
    inner_pipe = bpy.context.object
    inner_pipe.name = "InnerPipe"

    # Subtract inner_pipe from outer_pipe via Boolean modifier
    bool_mod = outer_pipe.modifiers.new(name="PipeHole", type="BOOLEAN")
    bool_mod.operation = "DIFFERENCE"
    bool_mod.object = inner_pipe
    bpy.context.view_layer.objects.active = outer_pipe
    bpy.ops.object.modifier_apply(modifier=bool_mod.name)

    bpy.data.objects.remove(inner_pipe, do_unlink=True)
    return outer_pipe


def assign_pipe_material(pipe):
    pipe_mat = bpy.data.materials.new(name="PipeMaterial")
    pipe_mat.use_nodes = True
    nodes = pipe_mat.node_tree.nodes
    principled = nodes.get("Principled BSDF")
    # Set to a bright Mario-like green, non-metallic with a slight shine
    principled.inputs["Base Color"].default_value = (0.0, 0.6, 0.0, 1)
    principled.inputs["Metallic"].default_value = 0.0
    principled.inputs["Roughness"].default_value = 0.2
    pipe.data.materials.append(pipe_mat)


def create_text():
    bpy.ops.object.text_add(location=(0, -1.5, 1))
    text_obj = bpy.context.object
    text_obj.name = "LogoText"
    text_data = text_obj.data
    text_data.body = "PortRedirect"
    text_data.align_x = "CENTER"
    text_data.align_y = "CENTER"
    text_data.extrude = 0.15
    text_data.size = 1.6
    # Set a custom font (adjust the path as needed)
    font_path = os.path.join(
        os.path.expanduser("~"), "Library/Fonts", "Comic Code Bold.otf"
    )
    if os.path.exists(font_path):
        text_data.font = bpy.data.fonts.load(font_path)
    bpy.ops.object.convert(target="MESH")
    return bpy.context.object


def assign_text_material(text_obj):
    text_mat = bpy.data.materials.new(name="TextMaterial")
    text_mat.use_nodes = True
    nodes = text_mat.node_tree.nodes
    links = text_mat.node_tree.links
    output_node = nodes.get("Material Output")
    principled_text = nodes.get("Principled BSDF")

    # Set a reflective gold color (Mario title screen gold, hex #FFD700)
    principled_text.inputs["Base Color"].default_value = (1.0, 0.84, 0.0, 1)
    principled_text.inputs["Metallic"].default_value = 0.7
    principled_text.inputs["Roughness"].default_value = 0.1
    if "Clearcoat" in principled_text.inputs:
        principled_text.inputs["Clearcoat"].default_value = 0.2
    if "Clearcoat Roughness" in principled_text.inputs:
        principled_text.inputs["Clearcoat Roughness"].default_value = 0.05

    # Add a subtle noise texture for gold variation
    noise_tex = nodes.new(type="ShaderNodeTexNoise")
    noise_tex.inputs["Scale"].default_value = 100.0
    noise_tex.inputs["Detail"].default_value = 16.0

    color_ramp = nodes.new(type="ShaderNodeValToRGB")
    color_ramp.color_ramp.elements[0].color = (1.0, 0.84, 0.0, 1)
    color_ramp.color_ramp.elements[1].color = (0.9, 0.75, 0.0, 1)

    mix_shader = nodes.new(type="ShaderNodeMixRGB")
    mix_shader.blend_type = "MULTIPLY"
    mix_shader.inputs["Fac"].default_value = 0.2
    links.new(noise_tex.outputs["Fac"], color_ramp.inputs["Fac"])
    links.new(color_ramp.outputs["Color"], mix_shader.inputs[2])
    mix_shader.inputs[1].default_value = principled_text.inputs[
        "Base Color"
    ].default_value
    links.new(mix_shader.outputs["Color"], principled_text.inputs["Base Color"])

    text_obj.data.materials.append(text_mat)


def setup_lighting():
    # Create an Area Light with increased energy
    bpy.ops.object.light_add(type="AREA", location=(0, 0, 10))
    light_area = bpy.context.object
    light_area.data.energy = 2000
    light_area.data.size = 100

    # Create a Point Light with increased energy
    bpy.ops.object.light_add(type="POINT", location=(-5, -7, 10))
    light_point = bpy.context.object
    light_point.data.energy = 7000


def setup_camera():
    bpy.ops.object.camera_add(location=(0, -10, 10))
    camera = bpy.context.object
    camera.rotation_euler = (0.785, 0, 0)  # 45° downward tilt
    bpy.context.scene.camera = camera


def setup_world_background():
    if bpy.data.worlds:
        world = bpy.data.worlds[0]
    else:
        world = bpy.data.worlds.new("World")
    bpy.context.scene.world = world
    world.use_nodes = True
    bg_node = world.node_tree.nodes.get("Background")
    if bg_node:
        # Slightly darker Mario Blue (hex #2D70B3 => (0.18, 0.44, 0.70, 1))
        bg_node.inputs[0].default_value = (0.18, 0.44, 0.70, 1)
        bg_node.inputs[1].default_value = (
            0.2  # Increase the strength for more ambient light
        )
    else:
        raise Exception("i need a Background node")


def setup_render_settings(output_path):
    scene = bpy.context.scene
    scene.render.engine = "BLENDER_EEVEE_NEXT"
    if hasattr(scene, "eevee_next"):
        scene.eevee_next.use_bloom = True
        scene.eevee_next.bloom_intensity = 0.3  # increased intensity
        scene.eevee_next.bloom_threshold = 0.5  # lower threshold
        scene.eevee_next.bloom_radius = 6.5
    else:
        print("Eevee Next bloom settings not found, skipping bloom configuration.")

    scene.view_settings.view_transform = "Filmic"
    scene.view_settings.look = "None"
    scene.view_settings.exposure = 0.95

    scene.render.resolution_x = 1920
    scene.render.resolution_y = 1080
    scene.render.image_settings.file_format = "PNG"
    scene.render.filepath = output_path


def main():
    cleanup_scene()
    setup_random_seed(42)
    create_background_pipes()
    create_clouds()
    pipe = create_hollow_pipe()
    assign_pipe_material(pipe)
    logo_text = create_text()
    assign_text_material(logo_text)
    setup_lighting()
    setup_camera()
    setup_world_background()
    output_path = os.path.join(bpy.path.abspath("//"), "portredirect_logo.png")
    setup_render_settings(output_path)

    bpy.ops.render.render(write_still=True)
    print("Logo rendered and saved to", output_path)


main()
