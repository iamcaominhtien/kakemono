(function(){
  var doc=document, cv=doc.createElement('canvas').getContext('2d');
  function clamp(v,a,b){return Math.max(a,Math.min(b,v));}
  function deepest(el){var e=el;while(e.firstElementChild)e=e.firstElementChild;return e;}

  // Give a line of text the width it has in the PDF. The fonts of the page
  // are rarely the fonts of the PDF, so the natural width of a line differs
  // from its target by a few percent (more for Computer Modern or sans
  // fonts), and the difference is shared out like this:
  //  - a line wider than its target is set in a slightly smaller type (down
  //    to FIT_MIN_SCALE), so that letters never run into each other and the
  //    word spaces stay open;
  //  - a line narrower than its target gets a little letter spacing (the
  //    font is narrower all through), and only the lines the PDF justifies
  //    (data-j) stretch their word spaces for the rest. Centred and ragged
  //    lines have the word spaces of the font, never huge gaps.
  // A line still wider than its target at FIT_MIN_SCALE is set in a font
  // much narrower than any fallback (a condensed sans, a narrow monospace):
  // it keeps its size and is squeezed sideways onto its width instead.
  // Letter spacing is added after every letter, the last one included.
  // Rotated text is measured along its own baseline.
  // Combining marks (a tilde over a letter) take no spacing of their own and
  // are not counted as letters.
  var FIT_MIN_SCALE=0.88,   // smallest type scale used to narrow a line
      FIT_SHRINK_FROM=0.985,// lines at most this much too wide still keep their size
      WS_MIN=-0.06,         // word spacing range of a justified line, in em
      WS_MAX=0.7,
      WS_RAGGED_MAX=0.3,    // largest extra word space of a line that is not justified
      LS_MIN=-0.02,         // letter spacing range, in em
      LS_MAX=0.15,
      LS_JUSTIFIED_MAX=0.06,// letter spacing for a font that is narrower all through, in justified lines
      SQUEEZE_MIN=0.6;      // narrowest sideways squeeze of a line
  function width(el){return el.classList.contains('rt')?el.offsetWidth:el.getBoundingClientRect().width;}
  function measure(el){el.style.wordSpacing='';el.style.letterSpacing='';return width(el);}
  // Sets the type of a line to `k` times the size it has in the page.
  function scaleType(el,k){
    if(el.dataset.fs0===undefined)el.dataset.fs0=el.style.fontSize;
    var orig=el.dataset.fs0;
    if(k>=1)el.style.fontSize=orig;
    else el.style.fontSize=orig?'calc('+orig+' * '+k.toFixed(4)+')':(k*100).toFixed(2)+'%';
  }
  function fitAll(els,pageW){
    var items=[],i;
    for(i=0;i<els.length;i++){
      var el=els[i],target=Number.parseFloat(el.dataset.w)/100*pageW;
      if(!(target>0))continue;
      scaleType(el,1);
      var nat=measure(el);
      if(nat>0)items.push({el:el,target:target,nat:nat,just:el.dataset.j==='1'});
    }
    // The ratio target/natural of the tightest justified lines of the group
    // is the difference of the fonts; the other lines only stretch from it.
    var rs=items.filter(function(it){return it.just;}).map(function(it){return it.target/it.nat;}).sort(function(a,b){return a-b;});
    var base=rs.length>=3?rs[Math.floor(rs.length*0.15)]:null;
    items.forEach(function(it){apply(it,base);});
  }
  // Squeezes a line sideways by `k`. A line of a flowing block becomes an
  // inline block, with a line break after it in place of its own.
  function squeeze(el,k){
    var ln=el.classList.contains('ln');
    if(!ln&&!el.classList.contains('pl'))return false;
    el.classList.add('sq');
    el.style.transform='scaleX('+Math.max(k,SQUEEZE_MIN).toFixed(4)+')';
    if(ln&&!el.classList.contains('end')&&!el.nextElementSibling?.classList.contains('sqbr')){
      var br=doc.createElement('br');br.className='sqbr';el.after(br);
    }
    return true;
  }
  function apply(it,base){
    var el=it.el,fs=Number.parseFloat(getComputedStyle(el).fontSize),target=it.target,nat=it.nat;
    if(target/nat<FIT_MIN_SCALE&&squeeze(el,target/nat))return;
    var ratio=it.just&&base!==null?base:target/nat;
    if(ratio<FIT_SHRINK_FROM){
      var k=Math.max(ratio,FIT_MIN_SCALE);
      scaleType(el,k);
      nat=measure(el);fs=Number.parseFloat(getComputedStyle(el).fontSize);
    }
    var d=target-nat,txt=el.textContent,gaps=(txt.match(/ /g)||[]).length,n=Array.from(txt.replace(/[\u0300-\u036f\u20d0-\u20ff\ufe00-\ufe0f]/g,'')).length,ws=0,ls=0;
    if(it.just){
      if(base!==null&&base>1&&n>1){ls=clamp((base-1)*nat/n,0,LS_JUSTIFIED_MAX*fs);d-=ls*n;}
      if(gaps>0){ws=clamp(d/gaps,WS_MIN*fs,WS_MAX*fs);d-=ws*gaps;}
      if(Math.abs(d)>0.3&&n>1){ls+=clamp(d/n,LS_MIN*fs,LS_MAX*fs);}
    }else{
      if(Math.abs(d)>0.3&&n>1){ls=clamp(d/n,LS_MIN*fs,LS_MAX*fs);d-=ls*n;}
      if(gaps>0&&Math.abs(d)>0.3){ws=clamp(d/gaps,WS_MIN*fs,WS_RAGGED_MAX*fs);}
    }
    function set(){
      if(ws)el.style.wordSpacing=(ws/fs).toFixed(4)+'em';
      if(ls)el.style.letterSpacing=(ls/fs).toFixed(4)+'em';
    }
    set();
    // One correction from the width really obtained: letters that are
    // spaced as a cluster and glyphs of other fonts do not add up exactly.
    var rest=target-width(el);
    if(Math.abs(rest)>0.3){
      if(it.just&&gaps>0)ws=clamp(ws+rest/gaps,WS_MIN*fs,WS_MAX*fs);
      else if(n>1)ls=clamp(ls+rest/n,LS_MIN*fs,LS_MAX*fs);
      set();
    }
  }

  // A rebuilt table whose rows turned out taller than its region (fonts with
  // taller lines than the PDF's) is scaled down to fit the region, so that it
  // never runs over the text below it.
  var TABLE_OVERFLOW=1.03,CELL_MIN_SCALE=0.6,CELL_ROOM=0.96;
  // A table whose cells hold text wider than their columns (a narrower font
  // in the PDF) gets a smaller type, the same in every cell, so that no
  // text runs into the next column.
  function fitCells(tb){
    var t=tb.firstElementChild,k=1;
    t.style.fontSize='';
    tb.querySelectorAll('td,th').forEach(function(c){
      if(c.clientWidth>0&&c.scrollWidth>c.clientWidth*CELL_ROOM)k=Math.min(k,c.clientWidth*CELL_ROOM/c.scrollWidth);
    });
    if(k<1)t.style.fontSize=(Math.max(k,CELL_MIN_SCALE)*100).toFixed(1)+'%';
  }
  function fitTable(tb){
    fitCells(tb);
    var t=tb.firstElementChild;
    if(!t)return;
    t.style.transform='';t.style.width='';
    var box=tb.getBoundingClientRect().height,h=t.getBoundingClientRect().height;
    if(box>0&&h>box*TABLE_OVERFLOW){
      var k=box/h;
      t.style.width=(100/k).toFixed(3)+'%';
      t.style.transformOrigin='0 0';
      t.style.transform='scale('+k.toFixed(4)+')';
    }
  }

  // Distance of the baseline of an element from the top of the viewport,
  // measured with an empty inline box (exact whatever the fonts).
  function baselineOf(el){
    var probe=doc.createElement('span');
    probe.style.cssText='display:inline-block;width:0;height:0;vertical-align:baseline';
    el.appendChild(probe);var y=probe.getBoundingClientRect().bottom;el.removeChild(probe);return y;
  }

  // Glyphs that do not sit on the baseline in the usual way (big operators,
  // tall delimiters, radicals) are stretched onto the box their ink has in
  // the PDF.
  function fitInk(el,pr){
    var ink=el.dataset.ink.split(',').map(Number),cs=getComputedStyle(deepest(el)),fs=Number.parseFloat(cs.fontSize);
    cv.font=cs.fontStyle+' '+cs.fontWeight+' '+fs+'px '+cs.fontFamily;
    var m=cv.measureText(el.textContent),iw=m.actualBoundingBoxLeft+m.actualBoundingBoxRight,ih=m.actualBoundingBoxAscent+m.actualBoundingBoxDescent;
    if(!(iw>0.01&&ih>0.01))return;
    el.style.transform='';
    var er=el.getBoundingClientRect(),base=baselineOf(el)-er.top;
    var tx=pr.left+ink[0]/100*pr.width,ty=pr.top+ink[1]/100*pr.height,tw=(ink[2]-ink[0])/100*pr.width,th=(ink[3]-ink[1])/100*pr.height;
    var sx=clamp(tw/iw,0.15,10),sy=clamp(th/ih,0.15,10);
    var dx=(tx-er.left)+m.actualBoundingBoxLeft*sx,dy=(ty-er.top)-(base-m.actualBoundingBoxAscent)*sy;
    el.style.transform='translate('+(dx/fs).toFixed(4)+'em,'+(dy/fs).toFixed(4)+'em) scale('+sx.toFixed(4)+','+sy.toFixed(4)+')';
  }

  function clonesOf(fb){return doc.querySelectorAll('.fb.cont[data-of="'+fb.dataset.block+'"]');}
  var original=new WeakMap(),done=new WeakSet(),busy=false;

  function fitBlock(fb){
    if(done.has(fb))return;done.add(fb);
    // Text edited before the page was written flows in its boxes already.
    if(fb.classList.contains('ed')){reflow(fb);original.set(fb,fb.textContent);return;}
    var page=fb.closest('.page'),w=page.getBoundingClientRect().width,lines=fb.querySelectorAll('.ln');
    fitAll(lines,w);
    clonesOf(fb).forEach(function(c){
      var cl=c.querySelectorAll('.ln');
      for(var i=0;i<cl.length&&i<lines.length;i++){
        cl[i].style.wordSpacing=lines[i].style.wordSpacing;cl[i].style.letterSpacing=lines[i].style.letterSpacing;cl[i].style.fontSize=lines[i].style.fontSize;
        if(lines[i].classList.contains('sq'))squeeze(cl[i],Number.parseFloat(lines[i].style.transform.slice(7)));
      }
    });
    original.set(fb,fb.textContent);
  }
  function fitPage(page){
    if(page.dataset.fit)return;page.dataset.fit='1';
    var pr=page.getBoundingClientRect();
    page.querySelectorAll('.fb').forEach(function(fb){
      if(fb.classList.contains('cont')){var src=doc.querySelector('.fb[data-block="'+fb.dataset.of+'"]');if(src)fitBlock(src);}
      else fitBlock(fb);
    });
    fitAll(page.querySelectorAll('.pl, .rt, .g:not([data-ink])'),pr.width);
    page.querySelectorAll('.tb').forEach(fitTable);
    page.querySelectorAll('.g[data-ink]').forEach(function(g){fitInk(g,pr);});
  }

  // After its text changed, a block gives up the line breaks of the PDF and
  // flows inside its boxes, shrinking the type if the new text is longer.
  function reflow(fb){
    busy=true;
    var inner=fb.firstElementChild,boxes=[fb].concat([].slice.call(clonesOf(fb))),total=0;
    boxes.forEach(function(b){b.classList.add('rf');total+=b.getBoundingClientRect().height;});
    fb.querySelectorAll('.ln[data-fs0]').forEach(function(l){l.style.fontSize=l.dataset.fs0;});
    boxes.forEach(function(b){b.querySelectorAll('.ln.sq').forEach(function(l){l.style.transform='';});});
    var lh=Number.parseFloat(getComputedStyle(fb).lineHeight)||0,scale=1;
    // A block of one line stays on one line ("or" between two formulas):
    // its type is made smaller to fit the width instead.
    var one=boxes.length===1&&total<lh*1.5;
    boxes.forEach(function(b){b.classList.toggle('nw',one);});
    var over=one?function(){return inner.scrollWidth>inner.clientWidth+1;}:function(){return inner.scrollHeight>total+lh*0.3;};
    inner.style.fontSize='';
    while(scale>0.62&&over()){scale-=0.03;inner.style.fontSize=Math.round(scale*100)+'%';}
    boxes.slice(1).forEach(function(b){var ci=b.firstElementChild;ci.innerHTML=inner.innerHTML;ci.style.fontSize=inner.style.fontSize;});
    setTimeout(function(){busy=false;},0);
  }
  var mo=new MutationObserver(function(recs){
    if(busy)return;
    var changed=new Set();
    recs.forEach(function(r){
      var n=r.target.nodeType===1?r.target:r.target.parentElement;if(!n||!n.closest)return;
      var fb=n.closest('.fb');if(fb&&!fb.classList.contains('cont'))changed.add(fb);
      var tb=n.closest('.tb');if(tb)tb.classList.add('rf');
      var pl=n.closest('.pl');if(pl){pl.style.wordSpacing='';pl.style.letterSpacing='';pl.style.transform='';if(pl.dataset.fs0!==undefined)pl.style.fontSize=pl.dataset.fs0;}
    });
    changed.forEach(function(fb){if(original.has(fb)&&fb.textContent!==original.get(fb))reflow(fb);});
  });

  var pages=doc.querySelectorAll('.page');
  if('IntersectionObserver' in window){
    var io=new IntersectionObserver(function(es){es.forEach(function(e){if(e.isIntersecting){fitPage(e.target);io.unobserve(e.target);}});},{rootMargin:'1200px'});
    pages.forEach(function(p){io.observe(p);});
  }else{pages.forEach(fitPage);}
  var main=doc.querySelector('.pages');
  if(main)mo.observe(main,{subtree:true,childList:true,characterData:true});
  window.kakemonoFitAll=function(){pages.forEach(fitPage);};

  var root=doc.documentElement,zoom=1.5;
  function set(z){zoom=clamp(z,0.5,4);root.style.setProperty('--zoom',zoom);doc.body.classList.remove('fit');}
  function on(id,f){var e=doc.getElementById(id);if(e)e.addEventListener('click',f);}
  on('zin',function(){set(zoom*1.2);});
  on('zout',function(){set(zoom/1.2);});
  on('zfit',function(){doc.body.classList.add('fit');});
})();
