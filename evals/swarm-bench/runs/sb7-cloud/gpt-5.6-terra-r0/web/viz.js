(()=>{
  'use strict';
  let resetView=()=>{};
  const nodeNames=['Northstar','Visa gateway','Meridian core','Chase settlement','Payout batch','Card reserve','ACH intake','Fraud review','NACHA rail','Reconciliation','Exception queue','Operator review'];
  function init(){
    const canvas=document.querySelector('#payment-viz');if(!canvas||canvas.dataset.meridianReady)return;canvas.dataset.meridianReady='true';
    const gl=canvas.getContext('webgl2',{alpha:false,antialias:true,depth:false});
    if(!gl){canvas.replaceWith(Object.assign(document.createElement('div'),{className:'webgl-error',textContent:'WebGL 2 is unavailable in this browser.'}));return}
    const compile=(type,source)=>{const shader=gl.createShader(type);gl.shaderSource(shader,source);gl.compileShader(shader);if(!gl.getShaderParameter(shader,gl.COMPILE_STATUS))throw new Error(gl.getShaderInfoLog(shader)||'Shader compilation failed');return shader};
    const program=(vertex,fragment)=>{const p=gl.createProgram();gl.attachShader(p,compile(gl.VERTEX_SHADER,vertex));gl.attachShader(p,compile(gl.FRAGMENT_SHADER,fragment));gl.linkProgram(p);if(!gl.getProgramParameter(p,gl.LINK_STATUS))throw new Error(gl.getProgramInfoLog(p)||'Program link failed');return p};
    const nodeVertex=`#version 300 es
      in vec2 a_quad;in vec4 a_node;uniform vec2 u_camera;uniform float u_aspect;uniform float u_time;uniform float u_selected;out vec3 v_color;out vec2 v_quad;out float v_selected;
      void main(){float id=a_node.w;float pulse=.94+.06*sin(u_time*1.8+id*1.73);vec2 pos=a_node.xy-u_camera;pos.x/=u_aspect;gl_Position=vec4(pos+a_quad*(.052*pulse),0.,1.);v_quad=a_quad;v_selected=step(abs(id-u_selected),.1);v_color=id<6.?vec3(.10,.48,1.):id<10.?vec3(0.,.82,.62):vec3(1.,.35,.19);}`;
    const nodeFragment=`#version 300 es
      precision highp float;in vec3 v_color;in vec2 v_quad;in float v_selected;out vec4 color;
      void main(){float d=length(v_quad);if(d>.5)discard;float edge=smoothstep(.5,.39,d);vec3 c=mix(v_color,vec3(1.),edge*.28);float ring=smoothstep(.5,.45,d)-smoothstep(.45,.40,d);c=mix(c,vec3(1.),ring*v_selected);color=vec4(c,1.);}`;
    const pickVertex=`#version 300 es
      in vec2 a_quad;in vec4 a_node;uniform vec2 u_camera;uniform float u_aspect;out float v_id;void main(){vec2 p=a_node.xy-u_camera;p.x/=u_aspect;gl_Position=vec4(p+a_quad*.06,0.,1.);v_id=a_node.w+1.;}`;
    const pickFragment=`#version 300 es
      precision highp float;in float v_id;out vec4 color;void main(){float d=length(gl_FragCoord.xy-floor(gl_FragCoord.xy)-.5);color=vec4(v_id/255.,0.,0.,1.);}`;
    const lineVertex=`#version 300 es
      in vec2 a_point;uniform vec2 u_camera;uniform float u_aspect;void main(){vec2 p=a_point-u_camera;p.x/=u_aspect;gl_Position=vec4(p,0.,1.);}`;
    const lineFragment=`#version 300 es
      precision mediump float;out vec4 color;void main(){color=vec4(.15,.39,.63,.75);}`;
    let nodes=new Float32Array([-.72,-.38,.8,0,-.46,.22,.7,1,-.16,-.08,.9,2,.12,.35,.8,3,.42,-.19,.9,4,.72,.18,.7,5,-.61,.49,.7,6,-.13,.57,.6,7,.35,.59,.6,8,.69,.48,.7,9,-.30,-.62,.5,10,.28,-.55,.5,11]);
    const links=new Float32Array([-.72,-.38,-.46,.22,-.46,.22,-.16,-.08,-.16,-.08,.12,.35,.12,.35,.42,-.19,.42,-.19,.72,.18,-.61,.49,-.13,.57,-.13,.57,.35,.59,.35,.59,.69,.48,-.30,-.62,.28,-.55,-.16,-.08,-.30,-.62,.42,-.19,.28,-.55]);
    const nodeProgram=program(nodeVertex,nodeFragment),lineProgram=program(lineVertex,lineFragment),pickProgram=program(pickVertex,pickFragment);
    const buffer=data=>{const b=gl.createBuffer();gl.bindBuffer(gl.ARRAY_BUFFER,b);gl.bufferData(gl.ARRAY_BUFFER,data,gl.STATIC_DRAW);return b};
    const quad=buffer(new Float32Array([-.5,-.5,.5,-.5,-.5,.5,.5,.5])),instance=buffer(nodes),line=buffer(links);
    const pickTexture=gl.createTexture(),pickFramebuffer=gl.createFramebuffer();let width=0,height=0,camera=[0,0],selected=-1,drag=null,raf=0;
    function resize(){const bounds=canvas.getBoundingClientRect(),ratio=Math.min(devicePixelRatio||1,2),nextWidth=Math.max(1,Math.round(bounds.width*ratio)),nextHeight=Math.max(1,Math.round(bounds.height*ratio));if(nextWidth===width&&nextHeight===height)return false;width=nextWidth;height=nextHeight;canvas.width=width;canvas.height=height;gl.viewport(0,0,width,height);gl.bindTexture(gl.TEXTURE_2D,pickTexture);gl.texImage2D(gl.TEXTURE_2D,0,gl.RGBA,width,height,0,gl.RGBA,gl.UNSIGNED_BYTE,null);gl.texParameteri(gl.TEXTURE_2D,gl.TEXTURE_MIN_FILTER,gl.NEAREST);gl.texParameteri(gl.TEXTURE_2D,gl.TEXTURE_MAG_FILTER,gl.NEAREST);gl.bindFramebuffer(gl.FRAMEBUFFER,pickFramebuffer);gl.framebufferTexture2D(gl.FRAMEBUFFER,gl.COLOR_ATTACHMENT0,gl.TEXTURE_2D,pickTexture,0);gl.bindFramebuffer(gl.FRAMEBUFFER,null);return true}
    function attributes(p){gl.bindBuffer(gl.ARRAY_BUFFER,quad);let attr=gl.getAttribLocation(p,'a_quad');gl.enableVertexAttribArray(attr);gl.vertexAttribPointer(attr,2,gl.FLOAT,false,0,0);gl.vertexAttribDivisor(attr,0);gl.bindBuffer(gl.ARRAY_BUFFER,instance);attr=gl.getAttribLocation(p,'a_node');gl.enableVertexAttribArray(attr);gl.vertexAttribPointer(attr,4,gl.FLOAT,false,0,0);gl.vertexAttribDivisor(attr,1)}
    function drawPick(){const aspect=width/height;gl.bindFramebuffer(gl.FRAMEBUFFER,pickFramebuffer);gl.viewport(0,0,width,height);gl.clearColor(0,0,0,0);gl.clear(gl.COLOR_BUFFER_BIT);gl.useProgram(pickProgram);attributes(pickProgram);gl.uniform2f(gl.getUniformLocation(pickProgram,'u_camera'),camera[0],camera[1]);gl.uniform1f(gl.getUniformLocation(pickProgram,'u_aspect'),aspect);gl.drawArraysInstanced(gl.TRIANGLE_STRIP,0,4,12);gl.bindFramebuffer(gl.FRAMEBUFFER,null)}
    function draw(time){resize();const aspect=width/height;gl.viewport(0,0,width,height);gl.clearColor(.035,.105,.175,1);gl.clear(gl.COLOR_BUFFER_BIT);gl.useProgram(lineProgram);gl.bindBuffer(gl.ARRAY_BUFFER,line);const lineAttr=gl.getAttribLocation(lineProgram,'a_point');gl.enableVertexAttribArray(lineAttr);gl.vertexAttribPointer(lineAttr,2,gl.FLOAT,false,0,0);gl.uniform2f(gl.getUniformLocation(lineProgram,'u_camera'),camera[0],camera[1]);gl.uniform1f(gl.getUniformLocation(lineProgram,'u_aspect'),aspect);gl.drawArrays(gl.LINES,0,links.length/2);gl.useProgram(nodeProgram);attributes(nodeProgram);gl.uniform2f(gl.getUniformLocation(nodeProgram,'u_camera'),camera[0],camera[1]);gl.uniform1f(gl.getUniformLocation(nodeProgram,'u_aspect'),aspect);gl.uniform1f(gl.getUniformLocation(nodeProgram,'u_time'),time*.001);gl.uniform1f(gl.getUniformLocation(nodeProgram,'u_selected'),selected);gl.drawArraysInstanced(gl.TRIANGLE_STRIP,0,4,12);raf=requestAnimationFrame(draw)}
    function pick(event){drawPick();const rect=canvas.getBoundingClientRect(),x=Math.max(0,Math.min(width-1,Math.floor((event.clientX-rect.left)/rect.width*width))),y=Math.max(0,Math.min(height-1,Math.floor((rect.bottom-event.clientY)/rect.height*height)));const pixel=new Uint8Array(4);gl.bindFramebuffer(gl.FRAMEBUFFER,pickFramebuffer);gl.readPixels(x,y,1,1,gl.RGBA,gl.UNSIGNED_BYTE,pixel);gl.bindFramebuffer(gl.FRAMEBUFFER,null);return pixel[0]?pixel[0]-1:-1}
    function updateReadout(){const output=document.querySelector('#viz-selection');output.textContent=selected<0?'Drag to orbit · select a node':`${nodeNames[selected]} · node ${String(selected+1).padStart(2,'0')} selected`}
    canvas.addEventListener('pointerdown',event=>{drag={x:event.clientX,y:event.clientY,camera:[...camera],moved:false};canvas.setPointerCapture(event.pointerId)});canvas.addEventListener('pointermove',event=>{if(!drag)return;const rect=canvas.getBoundingClientRect(),dx=event.clientX-drag.x,dy=event.clientY-drag.y;if(Math.abs(dx)+Math.abs(dy)>3)drag.moved=true;camera[0]=drag.camera[0]-dx/rect.width*2;camera[1]=drag.camera[1]+dy/rect.height*2});canvas.addEventListener('pointerup',event=>{if(drag&&!drag.moved){selected=pick(event);updateReadout()}drag=null;if(canvas.hasPointerCapture(event.pointerId))canvas.releasePointerCapture(event.pointerId)});canvas.addEventListener('pointercancel',()=>{drag=null});
    const observer=new ResizeObserver(()=>resize());observer.observe(canvas);resetView=()=>{camera=[0,0];selected=-1;updateReadout()};draw(0);
    window.addEventListener('pagehide',()=>{cancelAnimationFrame(raf);observer.disconnect()},{once:true});
  }
  window.MeridianViz={init,reset:()=>resetView()};
})();
