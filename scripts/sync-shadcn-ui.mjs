import fs from 'node:fs/promises';
import path from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const require = createRequire(path.join(root, 'frontend', 'package.json'));
const typescript = require('typescript');
const surfaces = [
  {
    directory: 'frontend', extension: 'tsx',
    components: ['button', 'card', 'input', 'label', 'dialog', 'skeleton'],
  },
  {
    directory: 'frontend/master-ui', extension: 'jsx',
    components: ['button', 'card', 'badge', 'input', 'textarea', 'label', 'alert-dialog', 'tabs', 'separator', 'skeleton'],
  },
  {
    directory: 'frontend/worker-ui', extension: 'jsx',
    components: ['button', 'card', 'badge', 'input', 'label', 'alert-dialog', 'tabs', 'separator', 'skeleton'],
  },
];
const components = [...new Set(surfaces.flatMap((surface) => surface.components))];
const utils = `import { clsx } from 'clsx';\nimport { twMerge } from 'tailwind-merge';\n\nexport function cn(...inputs) {\n  return twMerge(clsx(inputs));\n}\n`;

async function writeNewFile(filename, content) {
  await fs.mkdir(path.dirname(filename), { recursive: true });
  try {
    await fs.writeFile(filename, content, { flag: 'wx' });
    console.log(`Added ${path.relative(root, filename)}`);
  } catch (error) {
    if (error.code !== 'EEXIST') throw error;
    console.log(`Preserved ${path.relative(root, filename)}`);
  }
}

const theme = await fs.readFile(path.join(root, 'frontend/shadcn-theme.css'), 'utf8');
for (const surface of surfaces.filter((surface) => surface.extension === 'jsx')) {
  for (const filename of ['DesktopLifecycle.jsx', 'desktopLifecycle.mjs', 'desktop-lifecycle.css']) {
    const content = await fs.readFile(path.join(root, 'frontend/desktop', filename), 'utf8');
    await writeNewFile(path.join(root, surface.directory, 'src', filename), content);
  }
}
for (const surface of surfaces) {
  const utilsContent = surface.extension === 'tsx'
    ? 'export { cn } from "./utils";\n'
    : utils;
  await writeNewFile(path.join(root, surface.directory, 'src/lib/shadcn-utils.' + (surface.extension === 'tsx' ? 'ts' : 'js')), utilsContent);
  await writeNewFile(path.join(root, surface.directory, 'src/theme.css'), theme);
}

for (const name of components) {
  const response = await fetch(`https://ui.shadcn.com/r/styles/new-york/${name}.json`, {
    signal: AbortSignal.timeout(30_000),
  });
  if (!response.ok) throw new Error(`shadcn registry ${name}: HTTP ${response.status}`);
  const item = await response.json();
  if (item.name !== name || item.type !== 'registry:ui') throw new Error(`Unexpected registry item: ${name}`);
  const source = item.files.find((file) => file.path === `ui/${name}.tsx`)?.content;
  if (!source) throw new Error(`Missing component source: ${name}`);
  for (const surface of surfaces.filter((surface) => surface.components.includes(name))) {
    let content = source.replaceAll('@/lib/utils', '@/lib/shadcn-utils')
      .replaceAll('@/registry/new-york/ui/', '@/components/ui/');
    if (!content.startsWith('"use client"') && !content.startsWith("'use client'")) {
      content = `"use client"\n\n${content}`;
    }
    if (surface.extension === 'jsx') {
      content = typescript.transpileModule(content, {
        compilerOptions: {
          jsx: typescript.JsxEmit.Preserve,
          target: typescript.ScriptTarget.ES2022,
          module: typescript.ModuleKind.ESNext,
          verbatimModuleSyntax: true,
        },
        fileName: `${name}.tsx`,
      }).outputText;
    }
    await writeNewFile(path.join(root, surface.directory, 'src/components/ui', `${name}.${surface.extension}`), content);
  }
}
