const fs = require('fs');
const path = require('path');
const ts = require(process.env.TYPESCRIPT_PATH || 'typescript');
const root = path.resolve(__dirname, '..');
const files = fs.readdirSync(path.join(root,'src/react')).filter(f => f.endsWith('.jsx'));
const errors = [];
for (const file of files) {
  const result = ts.transpileModule(fs.readFileSync(path.join(root,'src/react',file),'utf8'), {
    fileName:file, reportDiagnostics:true,
    compilerOptions:{jsx:ts.JsxEmit.ReactJSX,target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ESNext}
  });
  for (const d of result.diagnostics || []) if(d.category === ts.DiagnosticCategory.Error)
    errors.push({file,message:ts.flattenDiagnosticMessageText(d.messageText,' ')});
}
fs.writeFileSync(path.join(root,'tests/syntax-results.json'),
  JSON.stringify({files:files.length,passed:errors.length===0,scope:'JSX syntax/transpilation, not a mounted React integration',errors},null,2)+'\n');
if(errors.length) { console.error(errors);process.exitCode=1; }
else console.log(files.length+' React JSX files parsed and transpiled.');
