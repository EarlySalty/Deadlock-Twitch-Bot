const fs=require('fs');
const path=require('path');
const tailwindPath=process.env.TAILWIND_PATH || path.dirname(require.resolve('tailwindcss/package.json'));
const tw=require(path.join(tailwindPath,'dist/lib.js'));
(async()=>{
 const root=__dirname;
 const files=[path.join(root,'index.html'),...fs.readdirSync(path.join(root,'src'),{recursive:true}).filter(f=>/\.(jsx|js)$/.test(f)).map(f=>path.join(root,'src',f))];
 const source=files.map(f=>fs.readFileSync(f,'utf8')).join('\n');
 const candidates=[...new Set(source.split(/[\s"'`<>;{}]+/).map(c=>c.replace(/^[,=]+|[,=]+$/g,'')).filter(Boolean))];
 const css='@layer theme, base, components, utilities;\n'+fs.readFileSync(path.join(tailwindPath,'theme.css'),'utf8')+'\n'+fs.readFileSync(path.join(root,'src/brand-theme.css'),'utf8')+'\n@layer base{'+fs.readFileSync(path.join(tailwindPath,'preflight.css'),'utf8')+'}\n@tailwind utilities;';
 const compiler=await tw.compile(css);
 fs.mkdirSync(path.join(root,'dist'),{recursive:true});fs.writeFileSync(path.join(root,'dist/tailwind.css'),compiler.build(candidates));
 const js=['model.js','icons.js','app.js'].map(f=>fs.readFileSync(path.join(root,'src',f),'utf8').replace(/^import .*;\n/gm,'').replace(/^export /gm,'')).join('\n');
 const html=fs.readFileSync(path.join(root,'index.html'),'utf8').replace('<link rel="stylesheet" href="./dist/tailwind.css"><link rel="stylesheet" href="./src/styles.css">',()=>'<style>'+compiler.build(candidates)+'\n'+fs.readFileSync(path.join(root,'src/styles.css'),'utf8').replace(/@font-face\s*\{[^}]*\}/g,'')+'</style>').replace('<script type="module" src="./src/app.js"></script>',()=>'<script type="module">\n'+('const esc = escapeHTML;\n'+js).replace(/<\/script/gi,'<\\/script')+'\n</script>');
 const logo='data:image/webp;base64,'+fs.readFileSync(path.join(root,'assets/deadlock-d-logo.webp')).toString('base64');
 fs.writeFileSync(path.join(root,'preview.html'),html.replaceAll('./assets/deadlock-d-logo.webp',logo));
 console.log('Built offline preview and Tailwind stylesheet. Classes:',candidates.length);
})();
