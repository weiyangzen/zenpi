export async function resolve(specifier,context,nextResolve) {
 if(/\/renderers\/(bash|grep|find)\.ts$/.test(specifier))return {url:new URL('./renderer-bridge.mjs',import.meta.url).href,shortCircuit:true};
 if(specifier==='typebox'||specifier.startsWith('typebox/'))return nextResolve(specifier,{...context,parentURL:new URL('./runtime/anchor.mjs',import.meta.url).href});
 return nextResolve(specifier,context);
}

