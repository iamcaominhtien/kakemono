// Added notes: a tap (or Enter) on a call shows its note in a popup next to it; a tap elsewhere, Escape or scrolling hides it.
(function(){var pop=document.createElement('div'),cur=null;pop.className='an-pop';pop.hidden=true;pop.setAttribute('role','note');document.body.appendChild(pop);
  function hide(){pop.hidden=true;cur=null}
  function show(t){var c=t.querySelector('.an-c');if(!c)return;pop.innerHTML=c.innerHTML;pop.hidden=false;cur=t;
    var r=t.getBoundingClientRect(),w=pop.offsetWidth,h=pop.offsetHeight,x=Math.max(8,Math.min(r.left,innerWidth-w-8)),y=r.bottom+6;
    if(y+h>innerHeight-8)y=Math.max(8,r.top-h-6);pop.style.left=x+'px';pop.style.top=y+'px'}
  document.addEventListener('click',function(e){var t=e.target.closest&&e.target.closest('.an');if(t){e.preventDefault();if(t===cur)hide();else show(t)}else if(!pop.contains(e.target))hide()});
  document.addEventListener('keydown',function(e){var a=document.activeElement;if(e.key==='Escape')hide();else if((e.key==='Enter'||e.key===' ')&&a&&a.classList.contains('an')){e.preventDefault();show(a)}});
  addEventListener('scroll',hide,{passive:true})})();
