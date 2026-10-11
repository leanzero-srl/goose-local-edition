import { widgetEdit } from '@forge/dashboards-bridge';
import { boot,call } from '../../shared/bridge';
import { escaped } from '../../shared/dom';
import '../../shared/base.css';import './edit.css';
const root=document.getElementById('root');
boot(async context=>{const stored=context?.extension?.config??{};let selected=stored.boardId==null?null:String(stored.boardId);
 await widgetEdit.onProductSave(async config=>selected?{...(config??{}),boardId:selected}:null);
 try{const data=await call('boards');if(data.error)throw new Error(data.error);
 function render(){root.innerHTML=`<div data-testid="scope-widget-edit" class="edit"><h2>Scrum board</h2><ul class="board-list">${data.boards.map(b=>`<li><button type="button" data-testid="board-option" data-board-id="${escaped(b.id)}" aria-pressed="${selected===b.id}" class="board-option${selected===b.id?' selected':''}"><span class="board-name">${escaped(b.name)}</span><span class="board-id">${escaped(b.id)}</span></button></li>`).join('')}</ul></div>`;for(const button of root.querySelectorAll('[data-board-id]'))button.addEventListener('click',async()=>{selected=button.dataset.boardId;await widgetEdit.updateConfig({...stored,boardId:selected});render();});}render();
 }catch(e){root.innerHTML=`<p class="message error" role="alert">${escaped(e.message)}</p>`;}
});
