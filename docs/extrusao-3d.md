# Extrusão 3D e contornos editáveis

Este guia descreve o que esta compilação do PhotoCraft faz de fato. Não é uma cópia do Photoshop CS6 nem do CorelDRAW, e não usa código nem arquivos desses produtos.

## Compilar no Linux x86_64

É preciso o Rust 1.95 ou mais novo (`rust-version` do projeto).

```sh
cargo build -p photocraft --release
```

O executável fica em `target/release/photocraft`. Abra-o no ambiente gráfico (wayland ou x11). Não há instalador nesta entrega: o arquivo é o binário nativo.

## Extrusão

Menu **3D**:

- **Nova extrusão 3D da camada** — camada de forma ou de texto. O contorno continua na camada. Com `live`, editar os nós e voltar a renderizar acompanha o traço.
- **Nova extrusão 3D do demarcador de trabalho** — cria uma forma editável a partir do demarcador e extrude.
- **Nova extrusão 3D da seleção** — traça a seleção. Uma seleção só de pixels não recupera curvas que o traçado não viu.
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

Os mesmos passos são comandos (`extrude.fromLayer`, `extrude.fromPath`, `extrude.fromSelection`, `extrude.set`, `extrude.setMaterial`, `extrude.refresh`, `extrude.remove`, `extrude.info`), com parâmetros JSON, como o resto do motor. `extrude.setMaterial` com `"face":"side"` e `"source":"image"` recebe `path`, `data` (base64) ou `pixels`. `"source":"gradient"` recebe `gradient.angle` e `gradient.stops` (pelo menos dois). `"source":"solid"` usa `color`.

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

- PDF: cada página continua a ter uma camada de pré-visualização raster (`Page N preview`). Por cima dela ficam caminhos editáveis quando o ficheiro os desenha: preenchimento, traço e contornos de glifos cujo tipo embutido o leitor consegue ler. Um furo em regra par-ímpar fica no mesmo caminho. Limites, ditos também nos avisos da importação: no máximo 1500 caminhos por página e 4096 nós por caminho; imagens, padrões, máscaras suaves, glifos Type 3 e recortes ficam só na pré-visualização; anotações não viram caminhos; as 14 fontes padrão (Helvetica, Times, Courier, Symbol, ZapfDingbats) não são substituídas nesta compilação, por isso esse texto fica na pré-visualização. Uma página só com imagem, ou um PDF digitalizado, não ganha vetores inventados.
- CDR (CorelDRAW) não é lido. Abrir um ficheiro `.cdr`, ou um contentor RIFF com tipo `CDR`, devolve um erro e não cria uma imagem raster no lugar das curvas. O `libcdr` 0.1.7 é MPL-2.0, licença fora da lista permitida (ISC, MIT, BSD-2-Clause, BSD-3-Clause, Apache-2.0, OFL-1.1, Ubuntu-font-1.0, Bitstream-Vera, CC0-1.0, CC-BY-4.0, CC-BY-SA-4.0, LicenseRef-Public-Domain). A lista não foi alargada e o conversor não foi ligado. Nenhuma versão de CDR foi testada, porque o conversor não entra na aplicação. Exporte SVG ou PDF a partir do CorelDRAW.
- O que o CS6 tem e esta versão de teste não tem: chão, várias luzes editáveis na interface, pintura na malha, Substance, dobrar, torcer, afunilar e inflar. A extrusão tem uma luz infinita e luz ambiente. O painel diz isso. Não é paridade com o Photoshop.
- A pré-visualização 3D não tem o ambiente completo do CS6: sem chão, sem várias luzes na interface, sem Substance, sem pintura 3D na malha, sem dobra, torção, afunilamento ou inflar. Há uma luz infinita e luz ambiente.
- Efeitos de camada (sombra, etc.) continuam a usar o contorno 2D da forma, não a silhueta já extrudada.
- Não há afirmação de paridade numérica com o Photoshop.
