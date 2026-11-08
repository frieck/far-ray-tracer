import * as THREE from "three";
import { viewHalfSize } from "./orbit";
import type { Pose } from "./types";

export interface Look {
  color: [number, number, number];
  metalness: number;
  roughness: number;
  opacity: number;
}

type Shape =
  | { type: "sphere"; radius: number }
  | { type: "box"; size: [number, number, number] }
  | { type: "cylinder"; radius: number; height: number }
  | { type: "tube"; radius: number; inner_radius: number; height: number }
  | { type: "cone"; radius: number; height: number }
  | { type: "torus"; major_radius: number; minor_radius: number };

export interface PreviewScene {
  objects: { shape: Shape; center: number[]; axes: number[][]; look: Look }[];
  meshes: { positions: number[]; normals: number[]; look: Look }[];
  lights: { position: number[]; color: number[]; corners: number[][] }[];
}

const IMAGE_DISTANCE = 5;
const SEGMENTS = 48;
/** The engine's Z points into the screen and Three.js's out of it. */
const toThree = ([x, y, z]: number[]) => new THREE.Vector3(x, y, -z);

function material({ color, metalness, roughness, opacity }: Look) {
  return new THREE.MeshStandardMaterial({
    color: new THREE.Color(...color),
    metalness: metalness * 0.6,
    roughness,
    opacity,
    transparent: opacity < 1,
    side: THREE.DoubleSide,
  });
}

function geometry(shape: Shape): THREE.BufferGeometry {
  switch (shape.type) {
    case "sphere":
      return new THREE.SphereGeometry(shape.radius, SEGMENTS, SEGMENTS / 2);
    case "box":
      return new THREE.BoxGeometry(...shape.size);
    case "cylinder":
      return new THREE.CylinderGeometry(shape.radius, shape.radius, shape.height, SEGMENTS);
    case "cone":
      return new THREE.ConeGeometry(shape.radius, shape.height, SEGMENTS);
    case "tube": {
      const { radius, inner_radius: inner, height } = shape;
      const half = height / 2;
      const outline = [[inner, -half], [radius, -half], [radius, half], [inner, half], [inner, -half]];
      return new THREE.LatheGeometry(outline.map(([x, y]) => new THREE.Vector2(x, y)), SEGMENTS);
    }
    case "torus": {
      // Three.js puts the torus axis on Z, and the engine on Y.
      const torus = new THREE.TorusGeometry(shape.major_radius, shape.minor_radius, SEGMENTS / 2, SEGMENTS * 2);
      return torus.rotateX(Math.PI / 2);
    }
  }
}

/** Draws the scene's shapes in real time while the camera moves. It only draws on request. */
export class SceneView {
  private readonly renderer: THREE.WebGLRenderer;
  private readonly scene = new THREE.Scene();
  private readonly camera = new THREE.PerspectiveCamera();
  private readonly content = new THREE.Group();

  /** Throws if this machine has no WebGL. */
  constructor(canvas: HTMLCanvasElement) {
    this.renderer = new THREE.WebGLRenderer({ canvas, antialias: true });
    this.renderer.setClearColor(0x0a0b0e);
    this.scene.add(this.content);
    this.scene.add(new THREE.HemisphereLight(0xffffff, 0x30343c, 1.4));
  }

  load(preview: PreviewScene) {
    this.content.traverse((item) => {
      if (item instanceof THREE.Mesh) {
        item.geometry.dispose();
        (item.material as THREE.Material).dispose();
      }
    });
    this.content.clear();

    for (const { shape, center, axes, look } of preview.objects) {
      const mesh = new THREE.Mesh(geometry(shape), material(look));
      const [x, y, z] = axes.map(toThree);
      mesh.matrixAutoUpdate = false;
      mesh.matrix.makeBasis(x, y, z).setPosition(toThree(center));
      this.content.add(mesh);
    }
    for (const { positions, normals, look } of preview.meshes) {
      const shape = new THREE.BufferGeometry();
      const flip = (values: number[]) => Float32Array.from(values, (v, i) => (i % 3 === 2 ? -v : v));
      const [points, directions] = [flip(positions), flip(normals)];
      faceAlongNormals(points, directions);
      shape.setAttribute("position", new THREE.BufferAttribute(points, 3));
      shape.setAttribute("normal", new THREE.BufferAttribute(directions, 3));
      this.content.add(new THREE.Mesh(shape, material(look)));
    }
    for (const { position, color, corners } of preview.lights) {
      const tint = new THREE.Color(...color.map((v) => Math.min(v, 1)) as [number, number, number]);
      const light = new THREE.PointLight(tint, 2.5, 0, 0);
      light.position.copy(toThree(position));
      this.content.add(light);
      this.content.add(corners.length ? panel(corners, tint) : marker(toThree(position), tint));
    }
  }

  draw(pose: Pose, aspect: number) {
    const canvas = this.renderer.domElement;
    this.renderer.setPixelRatio(window.devicePixelRatio);
    this.renderer.setSize(canvas.clientWidth, canvas.clientHeight, false);
    aimCamera(this.camera, pose, aspect);
    this.renderer.render(this.scene, this.camera);
  }
}

/** Gives a Three.js camera the same view as the engine at this pose. */
export function aimCamera(camera: THREE.PerspectiveCamera, pose: Pose, aspect: number) {
  const [, halfHeight] = viewHalfSize(aspect);
  camera.fov = THREE.MathUtils.radToDeg(2 * Math.atan(halfHeight / pose.zoom / IMAGE_DISTANCE));
  camera.aspect = aspect;
  camera.near = 0.05;
  camera.far = 5000;
  camera.position.copy(toThree(pose.position));
  camera.lookAt(toThree(pose.target));
  camera.updateProjectionMatrix();
  camera.updateMatrixWorld();
}

function marker(position: THREE.Vector3, color: THREE.Color) {
  const mesh = new THREE.Mesh(new THREE.SphereGeometry(0.12, 16, 8), new THREE.MeshBasicMaterial({ color }));
  mesh.position.copy(position);
  return mesh;
}

function panel(corners: number[][], color: THREE.Color) {
  const [a, b, c, d] = corners.map(toThree);
  const shape = new THREE.BufferGeometry().setFromPoints([a, b, c, a, c, d]);
  return new THREE.Mesh(shape, new THREE.MeshBasicMaterial({ color, side: THREE.DoubleSide }));
}

/**
 * Orders each triangle's corners so its front face points along its normal. Mirroring Z for
 * Three.js reverses the winding, and a back face is lit as if its normal pointed the other way,
 * which turned the ground black.
 */
function faceAlongNormals(points: Float32Array, normals: Float32Array) {
  const [a, b, c, n] = [new THREE.Vector3(), new THREE.Vector3(), new THREE.Vector3(), new THREE.Vector3()];
  for (let i = 0; i + 9 <= points.length; i += 9) {
    a.fromArray(points, i);
    b.fromArray(points, i + 3);
    c.fromArray(points, i + 6);
    n.fromArray(normals, i).add(n.clone().fromArray(normals, i + 3)).add(n.clone().fromArray(normals, i + 6));
    const face = b.sub(a).cross(c.sub(a));
    if (face.dot(n) >= 0) continue;
    for (const values of [points, normals]) {
      for (let k = 0; k < 3; k++) {
        [values[i + 3 + k], values[i + 6 + k]] = [values[i + 6 + k], values[i + 3 + k]];
      }
    }
  }
}
