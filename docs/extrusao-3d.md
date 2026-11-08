# Extrusão 3D e contornos editáveis

Este guia descreve o que esta compilação do PhotoCraft faz de fato. Não é uma cópia do Photoshop CS6 nem do CorelDRAW, e não usa código nem arquivos desses produtos.

## Pacote para Windows 10 x64

A entrega desta versão de teste é uma pasta portátil: `photocraft.exe` (janela) e `photocraft-cli.exe` (consola). Não há instalador nem pedido de administrador. Descompacte e abra `photocraft.exe`. Com `portable.txt` ao lado do executável, as preferências ficam na pasta `PhotoCraftData`, na mesma pasta, e não em `%APPDATA%`.

O binário foi cruzado a partir de Linux para `x86_64-pc-windows-gnu`, com o CRT do MinGW ligado dentro do executável (`-C target-feature=+crt-static`). Não é o MSI assinado que a integração contínua produz com MSVC e WiX. HEIC não entra neste pacote de teste. O formato PE (máquina `0x8664`, subsistema de janela 2, versão de subsistema 5.2) é o que se confere no ficheiro; isso não substitui uma execução num Windows 10 real.

Nesta compilação GNU, `windows.data.pdf.dll` é importada ao arrancar (a compilação MSVC adia essa carga). O Windows 10 traz essa DLL em `System32`. Se ela faltar, o programa não abre. Não é preciso o redistribuível do Visual C++: não há `libgcc`, `libstdc++` nem `libwinpthread` ao lado do executável.

## Compilar no Linux x86_64

Quem desenvolve neste repositório pode gerar o mesmo programa com Rust 1.95 ou mais novo:

```sh
cargo build -p photocraft --release
```

O executável fica em `target/release/photocraft`. Esse binário é Linux; não é o programa para Windows 10.

## Extrusão

Menu **3D**:

- **Nova extrusão 3D da camada selecionada** — forma vetorial ou texto com contorno. Uma camada de imagem é recusada. O contorno continua editável.
- **Nova extrusão 3D do demarcador selecionado** — cria uma forma a partir do demarcador de trabalho e extrude.
- Não há extrusão de uma seleção de pixels: isso traçaria um bitmap, e não faz parte desta função.
- **Atualizar extrusão 3D** — relê o contorno vivo (útil depois de editar texto).
- **Remover extrusão 3D** — a forma ou o texto ficam.

No painel Propriedades, com a extrusão já criada:

- profundidade;
- chanfro frontal e traseiro: largura, profundidade, segmentos e arredondamento (0 = chanfro reto, 1 = quarto de círculo). O chanfro é geometria, não um sombreado pintado;
- rotação do objeto e órbita da câmera;
- botão ortográfica;
- para cada superfície (frente, chanfro frontal, lateral, chanfro traseiro, verso): cor chapada, imagem ou degradê. Trocar a origem não apaga as outras;
- caminho de uma imagem PNG, JPEG ou outro formato que o PhotoCraft abre, aplicada à lateral. O comando também aceita arquivo, base64 ou pixels.

O mapeamento de cada superfície tem deslocamento, escala e rotação (`uv` no comando). Na lateral, U dá a volta no contorno e V desce na profundidade. No chanfro, V atravessa o perfil.

No canvas, o eixo no centro do sólido: arrastar orbita a câmera; Alt+arrastar gira o objeto. Há uma luz infinita e luz ambiente. Isso é uma pré-visualização sombreada, não um render com traçado de raios.

Os ajustes voltam no arquivo `.pcraft`, inclusive a imagem, o degradê e a última imagem renderizada. PSD não guarda este modelo 3D.

Os mesmos passos são comandos (`extrude.fromLayer`, `extrude.fromPath`, `extrude.set`, `extrude.setMaterial`, `extrude.refresh`, `extrude.remove`, `extrude.info`), com parâmetros JSON, como o resto do motor. `extrude.fromSelection` existe só para recusar o traçado de uma seleção de pixels; não aparece no menu. `extrude.setMaterial` com `"face":"side"` e `"source":"image"` recebe `path`, `data` (base64) ou `pixels`. `"source":"gradient"` recebe `gradient.angle` e `gradient.stops` (pelo menos dois). `"source":"solid"` usa `color`.

Um chanfro mais largo que a parede ou que o furo é reduzido até a malha fechar. Intersect, Exclude e demarcador invertido precisam de `path.mergeShapeComponents` antes.

## Forma, agrupar e nós

**Camada › Agrupar camadas** e **Camada › Desagrupar camadas** (atalho de desagrupar: Ctrl+Shift+G). Desagrupar abre um nível. A opacidade e a visibilidade do grupo passam para os filhos. Uma máscara só é copiada para o filho que não tinha. Um modo de mesclagem que não seja Pass Through é copiado para filhos que estavam em Normal. Subcaminhos, inclusive furos, continuam na mesma forma: desagrupar não separa um composto.

Um SVG com `<g id="...">` mantém o grupo, mesmo com um só filho, para poder desagrupar. Grupos sem nome que só carregam transformação continuam achatados: a transformação já está nos pontos. Texto de SVG vira contornos quando há fonte. Filtro, máscara complexa, padrão e imagem embutida podem virar pixels, com aviso, e não ficam editáveis como curva.

Menu **Shape** (comportamento parecido com a ferramenta Forma, implementação própria):

- fechar demarcador;
- quebrar num nó (caminho fechado abre; caminho aberto divide em dois e os dois ficam com o nó);
- juntar dois demarcadores abertos. Caminho fechado, inclusive furo, é recusado;
- segmento reto ou curvo;
- nó cúspide, suave ou simétrico.

A seleção direta, converter ponto e acrescentar ou apagar âncora já existiam nas ferramentas de demarcador (`path.moveAnchors`, `path.moveHandle`, `path.convertPoint`, `path.addAnchor`, `path.deleteAnchor`).

## O que esta entrega não faz

- A extrusão é só de vetor: uma forma, um demarcador ou o contorno real de um texto. Uma camada de imagem é recusada, com a frase em português na barra de estado. Uma seleção de pixels não é traçada. Imagens e degradês continuam a ser materiais da frente, do chanfro e da lateral.
- O contorno azul no modo de edição de caminho é o demarcador de origem visto na pose atual (o mesmo contorno da malha). A caixa de transformação do objeto, com a ferramenta Mover, continua a ser a caixa do sólido e não os nós.
- PDF: cada página continua a ter uma camada de pré-visualização raster (`Page N preview`). Por cima dela ficam caminhos editáveis quando o ficheiro os desenha: preenchimento, traço e contornos de glifos cujo tipo embutido o leitor consegue ler. Um furo em regra par-ímpar fica no mesmo caminho. Limites, ditos também nos avisos da importação: no máximo 1500 caminhos por página e 4096 nós por caminho; imagens, padrões, máscaras suaves, glifos Type 3 e recortes ficam só na pré-visualização; anotações não viram caminhos; as 14 fontes padrão (Helvetica, Times, Courier, Symbol, ZapfDingbats) não são substituídas nesta compilação, por isso esse texto fica na pré-visualização. Uma página só com imagem, ou um PDF digitalizado, não ganha vetores inventados.
- CDR (CorelDRAW) não é lido. Abrir um ficheiro `.cdr`, ou um contentor RIFF com tipo `CDR`, devolve um erro e não cria uma imagem raster no lugar das curvas. O `libcdr` 0.1.7 é MPL-2.0, licença fora da lista permitida (ISC, MIT, BSD-2-Clause, BSD-3-Clause, Apache-2.0, OFL-1.1, Ubuntu-font-1.0, Bitstream-Vera, CC0-1.0, CC-BY-4.0, CC-BY-SA-4.0, LicenseRef-Public-Domain). A lista não foi alargada e o conversor não foi ligado. Nenhuma versão de CDR foi testada, porque o conversor não entra na aplicação. Exporte SVG ou PDF a partir do CorelDRAW.
- O que o CS6 tem e esta versão de teste não tem: chão, várias luzes editáveis na interface, pintura na malha, Substance, dobrar, torcer, afunilar e inflar. A extrusão tem uma luz infinita e luz ambiente. O painel diz isso. Não é paridade com o Photoshop.
- Efeitos de camada (sombra, etc.) continuam a usar o contorno 2D da forma, não a silhueta já extrudada.
- Não há afirmação de paridade numérica com o Photoshop.
