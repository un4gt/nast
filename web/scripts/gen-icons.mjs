// 一次性生成 favicon 兜底图（favicon-32.png / apple-touch-icon.png / favicon.ico）。
// ICO 为 22 字节头 + 内嵌 PNG（Vista+ 通用做法）。
// 需先安装依赖再运行：npm i -D sharp && node scripts/gen-icons.mjs
// （产物已提交入库，常规构建无需 sharp。）
import sharp from 'sharp';
import { mkdirSync, writeFileSync } from 'node:fs';

const svg = 'public/favicon.svg';
mkdirSync('public', { recursive: true });

const png32 = await sharp(svg, { density: 300 }).resize(32, 32).png().toBuffer();
const png180 = await sharp(svg, { density: 300 }).resize(180, 180).png().toBuffer();

writeFileSync('public/favicon-32.png', png32);
writeFileSync('public/apple-touch-icon.png', png180);

// ICO 头：reserved(2)=0, type(2)=1, count(2)=1, entry(16), data
const head = Buffer.alloc(22);
head.writeUInt16LE(0, 0);      // reserved
head.writeUInt16LE(1, 2);      // type: icon
head.writeUInt16LE(1, 4);      // count
head.writeUInt8(32, 6);        // width
head.writeUInt8(32, 7);        // height
head.writeUInt8(0, 8);         // palette
head.writeUInt8(0, 9);         // reserved
head.writeUInt16LE(1, 10);     // color planes
head.writeUInt16LE(32, 12);    // bits per pixel
head.writeUInt32LE(png32.length, 14);
head.writeUInt32LE(22, 18);    // data offset
writeFileSync('public/favicon.ico', Buffer.concat([head, png32]));
console.log('icons generated');
