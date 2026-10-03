/**
 * 矩形树图布局的自检脚本（无需浏览器）。
 * 运行： npm run check:treemap
 */
import { buildTiles, squarify, type Rect } from "../src/lib/treemap";
import type { Node } from "../src/lib/api";

let failures = 0;

function check(name: string, cond: boolean, extra = "") {
  if (cond) {
    console.log(`  ✓ ${name}`);
  } else {
    failures += 1;
    console.error(`  ✗ ${name} ${extra}`);
  }
}

function area(r: Rect) {
  return Math.max(r.w, 0) * Math.max(r.h, 0);
}

function overlaps(a: Rect, b: Rect) {
  const eps = 0.01;
  return (
    a.x + a.w - eps > b.x &&
    b.x + b.w - eps > a.x &&
    a.y + a.h - eps > b.y &&
    b.y + b.h - eps > a.y
  );
}

console.log("1) squarify 基本性质");
{
  const weights = [6, 6, 4, 3, 2, 2, 1];
  const box: Rect = { x: 0, y: 0, w: 600, h: 400 };
  const rects = squarify(weights, box);

  check("返回数量与输入一致", rects.length === weights.length);
  check(
    "所有矩形都在容器内",
    rects.every((r) => r.x >= -0.01 && r.y >= -0.01 && r.x + r.w <= box.w + 0.01 && r.y + r.h <= box.h + 0.01),
  );

  let overlapFound = false;
  for (let i = 0; i < rects.length; i++) {
    for (let j = i + 1; j < rects.length; j++) {
      if (overlaps(rects[i], rects[j])) overlapFound = true;
    }
  }
  check("矩形之间互不重叠", !overlapFound);

  const totalArea = rects.reduce((a, r) => a + area(r), 0);
  const boxArea = box.w * box.h;
  check(
    `铺满容器（覆盖 ${((totalArea / boxArea) * 100).toFixed(1)}%）`,
    Math.abs(totalArea - boxArea) / boxArea < 0.01,
  );

  // 面积应与权重成正比
  const scale = boxArea / weights.reduce((a, b) => a + b, 0);
  const maxErr = Math.max(...rects.map((r, i) => Math.abs(area(r) - weights[i] * scale) / (weights[i] * scale)));
  check(`面积与权重成正比（最大误差 ${(maxErr * 100).toFixed(2)}%）`, maxErr < 0.02);
}

console.log("2) buildTiles 递归嵌套");
{
  const leaf = (name: string, size: number): Node => ({
    name,
    path: `/x/${name}`,
    size,
    isDir: false,
    files: 1,
    ext: "bin",
    modified: 0,
    children: [],
    truncated: false,
  });

  const dir = (name: string, size: number, children: Node[]): Node => ({
    name,
    path: `/root/${name}`,
    size,
    isDir: true,
    files: 1,
    modified: 0,
    children,
    truncated: false,
  });

  const root = dir("root", 100, [
    dir("big", 60, [leaf("a.bin", 40), leaf("b.bin", 20)]),
    dir("mid", 30, [leaf("c.bin", 30)]),
    leaf("d.bin", 10),
  ]);

  const box: Rect = { x: 0, y: 0, w: 1000, h: 700 };
  const tiles = buildTiles(root, box, { maxDepth: 3, minTileSize: 20 });

  check("生成了瓦片", tiles.length >= 6, `实际 ${tiles.length}`);
  check(
    "所有瓦片都在画布内",
    tiles.every((t) => t.rect.x >= -0.01 && t.rect.y >= -0.01 && t.rect.x + t.rect.w <= box.w + 0.01 && t.rect.y + t.rect.h <= box.h + 0.01),
  );

  const bigTile = tiles.find((t) => t.node.name === "big")!;
  const aInBig = tiles.find((t) => t.node.name === "a.bin")!;
  check("子瓦片包含在父瓦片范围内", aInBig.rect.x >= bigTile.rect.x - 0.01 && aInBig.rect.y >= bigTile.rect.y - 0.01 && aInBig.rect.x + aInBig.rect.w <= bigTile.rect.x + bigTile.rect.w + 0.01);
  check("父瓦片在子瓦片之前绘制", tiles.indexOf(bigTile) < tiles.indexOf(aInBig));

  const bigArea = area(bigTile.rect);
  const midArea = area(tiles.find((t) => t.node.name === "mid")!.rect);
  check(`面积排序正确（big ${bigArea.toFixed(0)} > mid ${midArea.toFixed(0)}）`, bigArea > midArea);

  const depthOk = tiles.every((t) => t.depth <= 3);
  check("递归深度受 maxDepth 限制", depthOk);
}

console.log("3) 边界情况");
{
  check("空权重返回空", squarify([], { x: 0, y: 0, w: 100, h: 100 }).length === 0);
  const zero = squarify([0, 0], { x: 0, y: 0, w: 100, h: 100 });
  check("全零权重不产生面积", zero.every((r) => area(r) === 0));
  const tiny = squarify([1, 1, 1], { x: 0, y: 0, w: 3, h: 3 });
  check("极小容器不崩溃", tiny.length === 3);
}

console.log("");
if (failures > 0) {
  console.error(`矩形树图布局自检失败：${failures} 项`);
  process.exit(1);
}
console.log("矩形树图布局自检全部通过 ✅");
