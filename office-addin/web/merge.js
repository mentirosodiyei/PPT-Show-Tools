export async function appendWithPowerPoint(files,{office=globalThis.Office,powerPoint=globalThis.PowerPoint,onProgress=()=>{},readBase64=readBlob}={}) {
  if(!office.context.requirements.isSetSupported('PowerPointApi','1.2'))throw new Error('Aquest PowerPoint no exposa PowerPointApi 1.2.');
  for(const file of files){
    const data=await readBase64(file);
    const event=await powerPoint.run(async context=>{
      const slides=context.presentation.slides;slides.load('items/id');await context.sync();
      const before=slides.items.map(x=>x.id);
      if(!before.length)throw new Error('Obre primer la còpia base preparada.');
      const last=before.at(-1);context.presentation.insertSlidesFromBase64(data,{formatting:'KeepSourceFormatting',targetSlideId:last.includes('#')?last:last+'#'});
      await context.sync();slides.load('items/id');await context.sync();
      const after=slides.items.map(x=>x.id),added=after.filter(x=>!before.includes(x));
      if(added.length!==file.slides||after.length!==before.length+file.slides)throw new Error(`${file.name}: importació no correcta; s’esperaven ${file.slides} diapositives noves i se’n detecten ${added.length}. No tornis a prémer importar sobre aquesta còpia parcial.`);
      if(before.some((id,i)=>after[i]!==id))throw new Error(`${file.name}: s’ha alterat l’ordre del destí.`);
      return {name:file.name,inserted:added.length,total:after.length,slideIds:added};
    });
    await onProgress(event);
  }
}
export async function readBlob(blob){return new Promise((resolve,reject)=>{const r=new FileReader();r.onload=()=>resolve(String(r.result).split(',')[1]);r.onerror=()=>reject(r.error);r.readAsDataURL(blob);});}
export async function exportPresentation(office=globalThis.Office){
  const file=await new Promise((resolve,reject)=>office.context.document.getFileAsync(office.FileType.Compressed,{sliceSize:1024*1024},r=>r.status===office.AsyncResultStatus.Succeeded?resolve(r.value):reject(new Error(r.error.message))));
  try{const chunks=[];for(let i=0;i<file.sliceCount;i++){const slice=await new Promise((resolve,reject)=>file.getSliceAsync(i,r=>r.status===office.AsyncResultStatus.Succeeded?resolve(r.value):reject(new Error(r.error.message))));chunks.push(new Uint8Array(slice.data));}return new Blob(chunks,{type:'application/octet-stream'});}finally{await new Promise(resolve=>file.closeAsync(resolve));}
}
