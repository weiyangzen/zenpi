export async function resolve(specifier,context,nextResolve) {
 if(specifier==='@earendil-works/pi-ai')return {url:new URL('./pi-ai-runtime.mjs',import.meta.url).href,shortCircuit:true};
 if(specifier==='typebox'||specifier.startsWith('typebox/'))return nextResolve(specifier,{...context,parentURL:new URL('./runtime/anchor.mjs',import.meta.url).href});
 return nextResolve(specifier,context);
}
