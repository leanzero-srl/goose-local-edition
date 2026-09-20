/* Generic WebGL setup only. Implement the payment scene, interaction and streaming below. */
'use strict';

globalThis.MeridianGL = (() => {
  function multiply(a, b) {
    const out = new Float32Array(16);
    for (let column = 0; column < 4; column++) {
      for (let row = 0; row < 4; row++) {
        for (let k = 0; k < 4; k++) out[column * 4 + row] += a[k * 4 + row] * b[column * 4 + k];
      }
    }
    return out;
  }
  function perspective(fovyRadians, aspect, near, far) {
    const f = 1 / Math.tan(fovyRadians / 2), range = 1 / (near - far);
    return new Float32Array([f / aspect, 0, 0, 0, 0, f, 0, 0, 0, 0,
      (far + near) * range, -1, 0, 0, 2 * far * near * range, 0]);
  }
  function lookAt(eye, target, up) {
    const unit = v => { const n = Math.hypot(...v); if (!n) throw Error('Degenerate camera basis'); return v.map(x => x / n); };
    const cross = (a, b) => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    const dot = (a, b) => a.reduce((sum, x, i) => sum + x * b[i], 0);
    const z = unit(eye.map((value, i) => value - target[i])), x = unit(cross(up, z)), y = cross(z, x);
    return new Float32Array([x[0], y[0], z[0], 0, x[1], y[1], z[1], 0,
      x[2], y[2], z[2], 0, -dot(x, eye), -dot(y, eye), -dot(z, eye), 1]);
  }
  function program(gl, vertexSource, fragmentSource) {
    const shaders = [], linked = gl.createProgram();
    try {
      for (const [type, source] of [[gl.VERTEX_SHADER, vertexSource], [gl.FRAGMENT_SHADER, fragmentSource]]) {
        const shader = gl.createShader(type); shaders.push(shader);
        gl.shaderSource(shader, source); gl.compileShader(shader);
        if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) throw Error(gl.getShaderInfoLog(shader));
        gl.attachShader(linked, shader);
      }
      gl.linkProgram(linked);
      if (!gl.getProgramParameter(linked, gl.LINK_STATUS)) throw Error(gl.getProgramInfoLog(linked));
      return linked;
    } catch (error) {
      gl.deleteProgram(linked); throw error;
    } finally {
      for (const shader of shaders) gl.deleteShader(shader);
    }
  }
  return { multiply, perspective, lookAt, program };
})();
