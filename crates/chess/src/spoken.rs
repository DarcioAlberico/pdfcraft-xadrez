//! Names an export says aloud, in the book's language (plan A9-X3b, finding R5-06): the annotation
//! signs, the pieces of a figurine and "to review" with the reason, for the HTML and EPUB a screen
//! reader reads and the names Word and RTF print after a sign.
//!
//! Every name is PdfCraft's own interface's, copied from its catalogues
//! (`crates/ui-egui/src/i18n/*.tsv`; a test in `ui-egui` keeps the copies equal to them) for the
//! languages whose prose the reader recognises and that have a catalogue: Portuguese, Spanish,
//! French, German, Italian, Russian and Polish ([`LANGS`]). Anything else is said in English, and
//! [`Said::lang`] says so: Dutch (recognised, but with no catalogue), the other notation languages,
//! and a name a catalogue lacks. A sign the interface does not name is said by the NAG table's
//! English name ([`crate::nag`]); none is translated here that the interface does not translate.
//!
//! Copied from the catalogues, never written here: when a catalogue's name changes, the test
//! `the_exports_spoken_names_are_the_catalogues` (`ui-egui`) names each copy to change with it.

use crate::nag;
use crate::placement::{Color, PieceKind};
use crate::san::Lang;

/// The languages a name is copied in, in the order of each row of the tables below.
pub const LANGS: [Lang; 7] = [Lang::Pt, Lang::Es, Lang::Fr, Lang::De, Lang::It, Lang::Ru, Lang::Pl];

/// A name and the language it is in: the book's, or English when there is none in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Said {
    pub text: &'static str,
    pub lang: Lang,
}

/// Why a move is to be reviewed: its status, as the Text tab names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    Illegal,
    Ambiguous,
    OutOfSequence,
    NotReached,
    Unchecked,
}

impl Reason {
    /// Every reason.
    pub const ALL: [Reason; 5] = [Reason::Illegal, Reason::Ambiguous, Reason::OutOfSequence, Reason::NotReached, Reason::Unchecked];
}

/// `english` in `lang` from a row of [`LANGS`]' translations; English when the row has none.
fn in_lang(english: &'static str, row: &[&'static str; 7], lang: Lang) -> Said {
    match LANGS.iter().position(|l| *l == lang).and_then(|i| row.get(i)).copied().filter(|t| !t.is_empty()) {
        Some(text) => Said { text, lang },
        None => Said { text: english, lang: Lang::En },
    }
}

/// The name of glyph `code` (either side's code), said in `lang`; `None` for a code the NAG
/// table does not have.
pub fn sign(code: u16, lang: Lang) -> Option<Said> {
    let entry = nag::by_code(code)?;
    Some(match SIGNS.iter().find(|(c, _, _)| *c == entry.code) {
        Some(&(_, english, ref row)) => in_lang(english, row, lang),
        None => Said { text: entry.name, lang: Lang::En },
    })
}

/// A piece of `color`, said in `lang` (`Cavalo branco`).
pub fn piece(color: Color, kind: PieceKind, lang: Lang) -> Said {
    let (english, row) = match (color, kind) {
        (Color::White, PieceKind::King) => ("White king", &PIECES[0]),
        (Color::White, PieceKind::Queen) => ("White queen", &PIECES[1]),
        (Color::White, PieceKind::Rook) => ("White rook", &PIECES[2]),
        (Color::White, PieceKind::Bishop) => ("White bishop", &PIECES[3]),
        (Color::White, PieceKind::Knight) => ("White knight", &PIECES[4]),
        (Color::White, PieceKind::Pawn) => ("White pawn", &PIECES[5]),
        (Color::Black, PieceKind::King) => ("Black king", &PIECES[6]),
        (Color::Black, PieceKind::Queen) => ("Black queen", &PIECES[7]),
        (Color::Black, PieceKind::Rook) => ("Black rook", &PIECES[8]),
        (Color::Black, PieceKind::Bishop) => ("Black bishop", &PIECES[9]),
        (Color::Black, PieceKind::Knight) => ("Black knight", &PIECES[10]),
        (Color::Black, PieceKind::Pawn) => ("Black pawn", &PIECES[11]),
    };
    in_lang(english, row, lang)
}

/// "To review", said in `lang`.
pub fn to_review(lang: Lang) -> Said {
    in_lang("To review", &TO_REVIEW, lang)
}

/// Why a move is to be reviewed, said in `lang`.
pub fn reason(reason: Reason, lang: Lang) -> Said {
    let (english, row) = match reason {
        Reason::Illegal => ("Illegal move", &REASONS[0]),
        Reason::Ambiguous => ("Ambiguous move", &REASONS[1]),
        Reason::OutOfSequence => ("Move out of sequence", &REASONS[2]),
        Reason::NotReached => ("Not reached: an earlier move failed", &REASONS[3]),
        Reason::Unchecked => ("Unchecked: no position to play it from", &REASONS[4]),
    };
    in_lang(english, row, lang)
}

/// Every name copied from a catalogue: its English text, the language and the copy. For the test
/// that keeps them equal to the catalogues.
pub fn copied() -> impl Iterator<Item = (&'static str, Lang, &'static str)> {
    let rows = SIGNS
        .iter()
        .map(|(_, english, row)| (*english, row))
        .chain(PIECE_ENGLISH.iter().copied().zip(PIECES.iter()))
        .chain(std::iter::once(("To review", &TO_REVIEW)))
        .chain(REASON_ENGLISH.iter().copied().zip(REASONS.iter()));
    rows.flat_map(|(english, row)| LANGS.iter().zip(row.iter()).filter(|(_, t)| !t.is_empty()).map(move |(l, t)| (english, *l, *t)))
}

/// Every English text of the tables, which the interface must have: for the same test.
pub fn english() -> impl Iterator<Item = &'static str> {
    SIGNS.iter().map(|(_, e, _)| *e).chain(PIECE_ENGLISH).chain(std::iter::once("To review")).chain(REASON_ENGLISH)
}

/// The signs the interface names, by the NAG table's code: the English name and its copies.
static SIGNS: [(u16, &str, [&str; 7]); 105] = [
    (1, "Good move", ["Bom lance", "Buena jugada", "Bon coup", "Guter Zug", "Buona mossa", "Хороший ход", "Dobry ruch"]),
    (2, "Poor move", ["Lance ruim", "Mala jugada", "Mauvais coup", "Schlechter Zug", "Mossa debole", "Слабый ход", "Słaby ruch"]),
    (
        3,
        "Brilliant move",
        ["Lance brilhante", "Jugada brillante", "Coup brillant", "Brillanter Zug", "Mossa brillante", "Блестящий ход", "Świetny ruch"],
    ),
    (4, "Blunder", ["Erro grave", "Error grave", "Gaffe", "Grober Fehler", "Grave errore", "Грубая ошибка", "Gruby błąd"]),
    (
        5,
        "Interesting move",
        ["Lance interessante", "Jugada interesante", "Coup intéressant", "Interessanter Zug", "Mossa interessante", "Интересный ход", "Ciekawy ruch"],
    ),
    (
        6,
        "Dubious move",
        ["Lance duvidoso", "Jugada dudosa", "Coup douteux", "Zweifelhafter Zug", "Mossa dubbia", "Сомнительный ход", "Wątpliwy ruch"],
    ),
    (7, "Only move", ["Lance único", "Única jugada", "Coup unique", "Einziger Zug", "Unica mossa", "Единственный ход", "Jedyny ruch"]),
    (146, "Novelty", ["Novidade", "Novedad", "Nouveauté", "Neuerung", "Novità", "Новинка", "Nowość"]),
    (
        144,
        "Equivalent move",
        [
            "Lance equivalente",
            "Jugada equivalente",
            "Coup équivalent",
            "Gleichwertiger Zug",
            "Mossa equivalente",
            "Равноценный ход",
            "Ruch równorzędny",
        ],
    ),
    (
        8,
        "Singular move",
        ["Único lance razoável", "Jugada forzada", "Coup forcé", "Erzwungener Zug", "Mossa obbligata", "Единственно разумный ход", "Ruch wymuszony"],
    ),
    (9, "Worst move", ["Pior lance", "Peor jugada", "Pire coup", "Schlechtester Zug", "Mossa peggiore", "Худший ход", "Najgorszy ruch"]),
    (
        10,
        "Equal position",
        ["Posição igual", "Posición igualada", "Position égale", "Ausgeglichene Stellung", "Posizione pari", "Равная позиция", "Pozycja równa"],
    ),
    (
        13,
        "Unclear position",
        [
            "Posição incerta",
            "Posición incierta",
            "Position incertaine",
            "Unklare Stellung",
            "Posizione poco chiara",
            "Неясная позиция",
            "Pozycja niejasna",
        ],
    ),
    (
        14,
        "White is slightly better",
        [
            "As brancas estão um pouco melhor",
            "Las blancas están algo mejor",
            "Les Blancs sont un peu mieux",
            "Weiß steht etwas besser",
            "Il Bianco sta leggermente meglio",
            "У белых небольшой перевес",
            "Białe stoją nieco lepiej",
        ],
    ),
    (
        15,
        "Black is slightly better",
        [
            "As pretas estão um pouco melhor",
            "Las negras están algo mejor",
            "Les Noirs sont un peu mieux",
            "Schwarz steht etwas besser",
            "Il Nero sta leggermente meglio",
            "У чёрных небольшой перевес",
            "Czarne stoją nieco lepiej",
        ],
    ),
    (
        16,
        "White is clearly better",
        [
            "As brancas estão claramente melhor",
            "Las blancas están claramente mejor",
            "Les Blancs sont nettement mieux",
            "Weiß steht deutlich besser",
            "Il Bianco sta nettamente meglio",
            "У белых явный перевес",
            "Białe stoją wyraźnie lepiej",
        ],
    ),
    (
        17,
        "Black is clearly better",
        [
            "As pretas estão claramente melhor",
            "Las negras están claramente mejor",
            "Les Noirs sont nettement mieux",
            "Schwarz steht deutlich besser",
            "Il Nero sta nettamente meglio",
            "У чёрных явный перевес",
            "Czarne stoją wyraźnie lepiej",
        ],
    ),
    (
        18,
        "White is winning",
        [
            "As brancas estão ganhando",
            "Las blancas ganan",
            "Les Blancs gagnent",
            "Weiß steht auf Gewinn",
            "Il Bianco vince",
            "Белые выигрывают",
            "Białe wygrywają",
        ],
    ),
    (
        19,
        "Black is winning",
        [
            "As pretas estão ganhando",
            "Las negras ganan",
            "Les Noirs gagnent",
            "Schwarz steht auf Gewinn",
            "Il Nero vince",
            "Чёрные выигрывают",
            "Czarne wygrywają",
        ],
    ),
    (
        44,
        "Compensation for the material",
        [
            "Compensação pelo material",
            "Compensación por el material",
            "Compensation pour le matériel",
            "Kompensation für das Material",
            "Compenso per il materiale",
            "Компенсация за материал",
            "Rekompensata za materiał",
        ],
    ),
    (
        11,
        "Equal chances, quiet position",
        [
            "Chances iguais, posição tranquila",
            "Igualdad, posición tranquila",
            "Chances égales, position calme",
            "Gleiche Chancen, ruhige Stellung",
            "Possibilità pari, posizione tranquilla",
            "Равные шансы, спокойная позиция",
            "Równe szanse, spokojna pozycja",
        ],
    ),
    (
        12,
        "Equal chances, active position",
        [
            "Chances iguais, posição ativa",
            "Igualdad, posición activa",
            "Chances égales, position active",
            "Gleiche Chancen, aktive Stellung",
            "Possibilità pari, posizione attiva",
            "Равные шансы, активная позиция",
            "Równe szanse, aktywna pozycja",
        ],
    ),
    (
        20,
        "White has a crushing advantage",
        [
            "As brancas têm vantagem esmagadora",
            "Las blancas tienen una ventaja aplastante",
            "Les Blancs ont un avantage écrasant",
            "Weiß hat erdrückenden Vorteil",
            "Il Bianco ha un vantaggio schiacciante",
            "У белых подавляющий перевес",
            "Białe mają miażdżącą przewagę",
        ],
    ),
    (
        21,
        "Black has a crushing advantage",
        [
            "As pretas têm vantagem esmagadora",
            "Las negras tienen una ventaja aplastante",
            "Les Noirs ont un avantage écrasant",
            "Schwarz hat erdrückenden Vorteil",
            "Il Nero ha un vantaggio schiacciante",
            "У чёрных подавляющий перевес",
            "Czarne mają miażdżącą przewagę",
        ],
    ),
    (
        42,
        "Insufficient compensation for the material",
        [
            "Compensação insuficiente pelo material",
            "Compensación insuficiente por el material",
            "Compensation insuffisante pour le matériel",
            "Ungenügende Kompensation für das Material",
            "Compenso insufficiente per il materiale",
            "Недостаточная компенсация за материал",
            "Niewystarczająca rekompensata za materiał",
        ],
    ),
    (
        46,
        "More than enough compensation for the material",
        [
            "Compensação mais que suficiente pelo material",
            "Compensación más que suficiente por el material",
            "Compensation plus que suffisante pour le matériel",
            "Mehr als genug Kompensation für das Material",
            "Compenso più che sufficiente per il materiale",
            "Более чем достаточная компенсация за материал",
            "Więcej niż wystarczająca rekompensata za materiał",
        ],
    ),
    (22, "Zugzwang", ["Zugzwang", "Zugzwang", "Zugzwang", "Zugzwang", "Zugzwang", "Цугцванг", "Zugzwang"]),
    (
        32,
        "Development advantage",
        [
            "Vantagem de desenvolvimento",
            "Ventaja en desarrollo",
            "Avance de développement",
            "Entwicklungsvorsprung",
            "Vantaggio di sviluppo",
            "Перевес в развитии",
            "Przewaga w rozwoju",
        ],
    ),
    (
        36,
        "With the initiative",
        ["Com a iniciativa", "Con la iniciativa", "Avec l'initiative", "Mit Initiative", "Con l'iniziativa", "С инициативой", "Z inicjatywą"],
    ),
    (40, "With an attack", ["Com ataque", "Con ataque", "Avec attaque", "Mit Angriff", "Con attacco", "С атакой", "Z atakiem"]),
    (
        132,
        "With counterplay",
        ["Com contrajogo", "Con contrajuego", "Avec contre-jeu", "Mit Gegenspiel", "Con controgioco", "С контригрой", "Z kontrgrą"],
    ),
    (138, "Time trouble", ["Apuro de tempo", "Apuros de tiempo", "Zeitnot", "Zeitnot", "Zeitnot", "Цейтнот", "Niedoczas"]),
    (
        30,
        "Slight development advantage",
        [
            "Leve vantagem de desenvolvimento",
            "Ligera ventaja en desarrollo",
            "Légère avance de développement",
            "Leichter Entwicklungsvorsprung",
            "Leggero vantaggio di sviluppo",
            "Небольшой перевес в развитии",
            "Niewielka przewaga w rozwoju",
        ],
    ),
    (
        34,
        "Decisive development advantage",
        [
            "Vantagem decisiva de desenvolvimento",
            "Ventaja decisiva en desarrollo",
            "Avance de développement décisive",
            "Entscheidender Entwicklungsvorsprung",
            "Vantaggio di sviluppo decisivo",
            "Решающий перевес в развитии",
            "Decydująca przewaga w rozwoju",
        ],
    ),
    (
        38,
        "Lasting initiative",
        [
            "Iniciativa duradoura",
            "Iniciativa duradera",
            "Initiative durable",
            "Anhaltende Initiative",
            "Iniziativa duratura",
            "Прочная инициатива",
            "Trwała inicjatywa",
        ],
    ),
    (
        130,
        "Slight counterplay",
        [
            "Leve contrajogo",
            "Ligero contrajuego",
            "Léger contre-jeu",
            "Leichtes Gegenspiel",
            "Leggero controgioco",
            "Небольшая контригра",
            "Niewielka kontrgra",
        ],
    ),
    (
        134,
        "Decisive counterplay",
        [
            "Contrajogo decisivo",
            "Contrajuego decisivo",
            "Contre-jeu décisif",
            "Entscheidendes Gegenspiel",
            "Controgioco decisivo",
            "Решающая контригра",
            "Decydująca kontrgra",
        ],
    ),
    (
        136,
        "Moderate time pressure",
        [
            "Zeitnot moderado",
            "Apuros de tiempo moderados",
            "Zeitnot modéré",
            "Mäßige Zeitnot",
            "Zeitnot moderato",
            "Небольшой цейтнот",
            "Umiarkowany niedoczas",
        ],
    ),
    (140, "With the idea", ["Com a ideia", "Con la idea", "Avec l'idée", "Mit der Idee", "Con l'idea", "С идеей", "Z ideą"]),
    (
        141,
        "Aimed against",
        ["Dirigido contra", "Dirigido contra", "Dirigé contre", "Gerichtet gegen", "Diretto contro", "Направлено против", "Skierowane przeciw"],
    ),
    (142, "Better is", ["Melhor é", "Mejor es", "Meilleur est", "Besser ist", "Meglio è", "Лучше", "Lepiej"]),
    (143, "Worse is", ["Pior é", "Peor es", "Moins bon est", "Schlechter ist", "Peggio è", "Хуже", "Gorzej"]),
    (
        145,
        "Editorial comment",
        [
            "Comentário editorial",
            "Comentario editorial",
            "Commentaire éditorial",
            "Redaktioneller Kommentar",
            "Commento editoriale",
            "Редакционный комментарий",
            "Komentarz redakcyjny",
        ],
    ),
    (254, "With", ["Com", "Con", "Avec", "Mit", "Con", "С", "Z"]),
    (255, "Without", ["Sem", "Sin", "Sans", "Ohne", "Senza", "Без", "Bez"]),
    (
        26,
        "Moderate space advantage",
        [
            "Vantagem de espaço moderada",
            "Ventaja de espacio moderada",
            "Avantage d'espace modéré",
            "Mäßiger Raumvorteil",
            "Vantaggio di spazio moderato",
            "Заметное преимущество в пространстве",
            "Umiarkowana przewaga przestrzenna",
        ],
    ),
    (
        238,
        "Space advantage",
        [
            "Vantagem de espaço",
            "Ventaja de espacio",
            "Avantage d'espace",
            "Raumvorteil",
            "Vantaggio di spazio",
            "Преимущество в пространстве",
            "Przewaga przestrzenna",
        ],
    ),
    (
        239,
        "Play along the file",
        [
            "Jogo pela coluna",
            "Juego por la columna",
            "Jeu sur la colonne",
            "Spiel auf der Linie",
            "Gioco sulla colonna",
            "Игра по вертикали",
            "Gra na linii",
        ],
    ),
    (
        240,
        "Play along the diagonal",
        [
            "Jogo pela diagonal",
            "Juego por la diagonal",
            "Jeu sur la diagonale",
            "Spiel auf der Diagonale",
            "Gioco sulla diagonale",
            "Игра по диагонали",
            "Gra na przekątnej",
        ],
    ),
    (
        241,
        "Play in the center",
        ["Jogo no centro", "Juego en el centro", "Jeu au centre", "Spiel im Zentrum", "Gioco al centro", "Игра в центре", "Gra w centrum"],
    ),
    (
        242,
        "Play on the kingside",
        [
            "Jogo na ala do rei",
            "Juego en el flanco de rey",
            "Jeu sur l'aile roi",
            "Spiel am Königsflügel",
            "Gioco sull'ala di re",
            "Игра на королевском фланге",
            "Gra na skrzydle królewskim",
        ],
    ),
    (
        243,
        "Play on the queenside",
        [
            "Jogo na ala da dama",
            "Juego en el flanco de dama",
            "Jeu sur l'aile dame",
            "Spiel am Damenflügel",
            "Gioco sull'ala di donna",
            "Игра на ферзевом фланге",
            "Gra na skrzydle hetmańskim",
        ],
    ),
    (244, "Weak point", ["Ponto fraco", "Punto débil", "Point faible", "Schwaches Feld", "Punto debole", "Слабый пункт", "Słabe pole"]),
    (245, "Endgame", ["Final", "Final", "Finale", "Endspiel", "Finale", "Эндшпиль", "Końcówka"]),
    (
        24,
        "Slight space advantage",
        [
            "Leve vantagem de espaço",
            "Ligera ventaja de espacio",
            "Léger avantage d'espace",
            "Leichter Raumvorteil",
            "Leggero vantaggio di spazio",
            "Небольшое преимущество в пространстве",
            "Niewielka przewaga przestrzenna",
        ],
    ),
    (
        28,
        "Decisive space advantage",
        [
            "Vantagem de espaço decisiva",
            "Ventaja de espacio decisiva",
            "Avantage d'espace décisif",
            "Entscheidender Raumvorteil",
            "Vantaggio di spazio decisivo",
            "Решающее преимущество в пространстве",
            "Decydująca przewaga przestrzenna",
        ],
    ),
    (
        48,
        "Slight control of the center",
        [
            "Leve controle do centro",
            "Ligero control del centro",
            "Léger contrôle du centre",
            "Leichte Kontrolle des Zentrums",
            "Leggero controllo del centro",
            "Небольшой контроль над центром",
            "Niewielka kontrola centrum",
        ],
    ),
    (
        50,
        "Moderate control of the center",
        [
            "Controle moderado do centro",
            "Control moderado del centro",
            "Contrôle modéré du centre",
            "Mäßige Kontrolle des Zentrums",
            "Controllo moderato del centro",
            "Заметный контроль над центром",
            "Umiarkowana kontrola centrum",
        ],
    ),
    (
        52,
        "Decisive control of the center",
        [
            "Controle decisivo do centro",
            "Control decisivo del centro",
            "Contrôle décisif du centre",
            "Entscheidende Kontrolle des Zentrums",
            "Controllo decisivo del centro",
            "Решающий контроль над центром",
            "Decydująca kontrola centrum",
        ],
    ),
    (
        54,
        "Slight control of the kingside",
        [
            "Leve controle da ala do rei",
            "Ligero control del flanco de rey",
            "Léger contrôle de l'aile roi",
            "Leichte Kontrolle des Königsflügels",
            "Leggero controllo dell'ala di re",
            "Небольшой контроль над королевским флангом",
            "Niewielka kontrola skrzydła królewskiego",
        ],
    ),
    (
        56,
        "Moderate control of the kingside",
        [
            "Controle moderado da ala do rei",
            "Control moderado del flanco de rey",
            "Contrôle modéré de l'aile roi",
            "Mäßige Kontrolle des Königsflügels",
            "Controllo moderato dell'ala di re",
            "Заметный контроль над королевским флангом",
            "Umiarkowana kontrola skrzydła królewskiego",
        ],
    ),
    (
        58,
        "Decisive control of the kingside",
        [
            "Controle decisivo da ala do rei",
            "Control decisivo del flanco de rey",
            "Contrôle décisif de l'aile roi",
            "Entscheidende Kontrolle des Königsflügels",
            "Controllo decisivo dell'ala di re",
            "Решающий контроль над королевским флангом",
            "Decydująca kontrola skrzydła królewskiego",
        ],
    ),
    (
        60,
        "Slight control of the queenside",
        [
            "Leve controle da ala da dama",
            "Ligero control del flanco de dama",
            "Léger contrôle de l'aile dame",
            "Leichte Kontrolle des Damenflügels",
            "Leggero controllo dell'ala di donna",
            "Небольшой контроль над ферзевым флангом",
            "Niewielka kontrola skrzydła hetmańskiego",
        ],
    ),
    (
        62,
        "Moderate control of the queenside",
        [
            "Controle moderado da ala da dama",
            "Control moderado del flanco de dama",
            "Contrôle modéré de l'aile dame",
            "Mäßige Kontrolle des Damenflügels",
            "Controllo moderato dell'ala di donna",
            "Заметный контроль над ферзевым флангом",
            "Umiarkowana kontrola skrzydła hetmańskiego",
        ],
    ),
    (
        64,
        "Decisive control of the queenside",
        [
            "Controle decisivo da ala da dama",
            "Control decisivo del flanco de dama",
            "Contrôle décisif de l'aile dame",
            "Entscheidende Kontrolle des Damenflügels",
            "Controllo decisivo dell'ala di donna",
            "Решающий контроль над ферзевым флангом",
            "Decydująca kontrola skrzydła hetmańskiego",
        ],
    ),
    (
        66,
        "Vulnerable first rank",
        [
            "Primeira fileira vulnerável",
            "Primera fila vulnerable",
            "Première rangée vulnérable",
            "Verwundbare Grundreihe",
            "Prima traversa vulnerabile",
            "Уязвимая первая горизонталь",
            "Słaba pierwsza linia",
        ],
    ),
    (
        68,
        "Well protected first rank",
        [
            "Primeira fileira bem protegida",
            "Primera fila bien protegida",
            "Première rangée bien protégée",
            "Gut geschützte Grundreihe",
            "Prima traversa ben protetta",
            "Хорошо защищённая первая горизонталь",
            "Dobrze chroniona pierwsza linia",
        ],
    ),
    (
        70,
        "Poorly protected king",
        [
            "Rei mal protegido",
            "Rey mal protegido",
            "Roi mal protégé",
            "Schlecht geschützter König",
            "Re mal protetto",
            "Плохо защищённый король",
            "Słabo chroniony król",
        ],
    ),
    (
        72,
        "Well protected king",
        [
            "Rei bem protegido",
            "Rey bien protegido",
            "Roi bien protégé",
            "Gut geschützter König",
            "Re ben protetto",
            "Хорошо защищённый король",
            "Dobrze chroniony król",
        ],
    ),
    (
        74,
        "Poorly placed king",
        [
            "Rei mal posicionado",
            "Rey mal situado",
            "Roi mal placé",
            "Schlecht postierter König",
            "Re mal piazzato",
            "Плохо расположенный король",
            "Źle ustawiony król",
        ],
    ),
    (
        76,
        "Well placed king",
        [
            "Rei bem posicionado",
            "Rey bien situado",
            "Roi bien placé",
            "Gut postierter König",
            "Re ben piazzato",
            "Хорошо расположенный король",
            "Dobrze ustawiony król",
        ],
    ),
    (
        249,
        "Connected pawns",
        ["Peões ligados", "Peones unidos", "Pions liés", "Verbundene Bauern", "Pedoni collegati", "Связанные пешки", "Piony połączone"],
    ),
    (
        250,
        "Isolated pawns",
        ["Peões isolados", "Peones aislados", "Pions isolés", "Isolierte Bauern", "Pedoni isolati", "Изолированные пешки", "Piony izolowane"],
    ),
    (
        251,
        "Doubled pawns",
        ["Peões dobrados", "Peones doblados", "Pions doublés", "Doppelbauern", "Pedoni doppiati", "Сдвоенные пешки", "Piony zdwojone"],
    ),
    (252, "Passed pawn", ["Peão passado", "Peón pasado", "Pion passé", "Freibauer", "Pedone passato", "Проходная пешка", "Wolny pion"]),
    (
        253,
        "Pawn majority",
        [
            "Maioria de peões",
            "Mayoría de peones",
            "Majorité de pions",
            "Bauernmehrheit",
            "Maggioranza di pedoni",
            "Пешечное большинство",
            "Przewaga pionów",
        ],
    ),
    (
        78,
        "Very weak pawn structure",
        [
            "Estrutura de peões muito fraca",
            "Estructura de peones muy débil",
            "Structure de pions très faible",
            "Sehr schwache Bauernstruktur",
            "Struttura pedonale molto debole",
            "Очень слабая пешечная структура",
            "Bardzo słaba struktura pionów",
        ],
    ),
    (
        80,
        "Moderately weak pawn structure",
        [
            "Estrutura de peões moderadamente fraca",
            "Estructura de peones moderadamente débil",
            "Structure de pions assez faible",
            "Ziemlich schwache Bauernstruktur",
            "Struttura pedonale abbastanza debole",
            "Довольно слабая пешечная структура",
            "Dość słaba struktura pionów",
        ],
    ),
    (
        82,
        "Moderately strong pawn structure",
        [
            "Estrutura de peões moderadamente forte",
            "Estructura de peones moderadamente fuerte",
            "Structure de pions assez solide",
            "Ziemlich starke Bauernstruktur",
            "Struttura pedonale abbastanza solida",
            "Довольно прочная пешечная структура",
            "Dość mocna struktura pionów",
        ],
    ),
    (
        84,
        "Very strong pawn structure",
        [
            "Estrutura de peões muito forte",
            "Estructura de peones muy fuerte",
            "Structure de pions très solide",
            "Sehr starke Bauernstruktur",
            "Struttura pedonale molto solida",
            "Очень прочная пешечная структура",
            "Bardzo mocna struktura pionów",
        ],
    ),
    (
        246,
        "Pair of bishops",
        ["Par de bispos", "Pareja de alfiles", "Paire de fous", "Läuferpaar", "Coppia degli alfieri", "Два слона", "Para gońców"],
    ),
    (
        247,
        "Bishops of opposite colors",
        [
            "Bispos de cores opostas",
            "Alfiles de distinto color",
            "Fous de couleurs opposées",
            "Ungleichfarbige Läufer",
            "Alfieri di colore contrario",
            "Разноцветные слоны",
            "Gońce różnopolowe",
        ],
    ),
    (
        248,
        "Bishops of the same color",
        [
            "Bispos da mesma cor",
            "Alfiles del mismo color",
            "Fous de même couleur",
            "Gleichfarbige Läufer",
            "Alfieri dello stesso colore",
            "Одноцветные слоны",
            "Gońce jednopolowe",
        ],
    ),
    (
        86,
        "Poor knight placement",
        [
            "Cavalo mal posicionado",
            "Caballo mal situado",
            "Cavalier mal placé",
            "Schlecht postierter Springer",
            "Cavallo mal piazzato",
            "Плохое расположение коня",
            "Źle ustawiony skoczek",
        ],
    ),
    (
        88,
        "Good knight placement",
        [
            "Cavalo bem posicionado",
            "Caballo bien situado",
            "Cavalier bien placé",
            "Gut postierter Springer",
            "Cavallo ben piazzato",
            "Хорошее расположение коня",
            "Dobrze ustawiony skoczek",
        ],
    ),
    (
        90,
        "Poor bishop placement",
        [
            "Bispo mal posicionado",
            "Alfil mal situado",
            "Fou mal placé",
            "Schlecht postierter Läufer",
            "Alfiere mal piazzato",
            "Плохое расположение слона",
            "Źle ustawiony goniec",
        ],
    ),
    (
        92,
        "Good bishop placement",
        [
            "Bispo bem posicionado",
            "Alfil bien situado",
            "Fou bien placé",
            "Gut postierter Läufer",
            "Alfiere ben piazzato",
            "Хорошее расположение слона",
            "Dobrze ustawiony goniec",
        ],
    ),
    (
        94,
        "Poor rook placement",
        [
            "Torre mal posicionada",
            "Torre mal situada",
            "Tour mal placée",
            "Schlecht postierter Turm",
            "Torre mal piazzata",
            "Плохое расположение ладьи",
            "Źle ustawiona wieża",
        ],
    ),
    (
        96,
        "Good rook placement",
        [
            "Torre bem posicionada",
            "Torre bien situada",
            "Tour bien placée",
            "Gut postierter Turm",
            "Torre ben piazzata",
            "Хорошее расположение ладьи",
            "Dobrze ustawiona wieża",
        ],
    ),
    (
        98,
        "Poor queen placement",
        [
            "Dama mal posicionada",
            "Dama mal situada",
            "Dame mal placée",
            "Schlecht postierte Dame",
            "Donna mal piazzata",
            "Плохое расположение ферзя",
            "Źle ustawiony hetman",
        ],
    ),
    (
        100,
        "Good queen placement",
        [
            "Dama bem posicionada",
            "Dama bien situada",
            "Dame bien placée",
            "Gut postierte Dame",
            "Donna ben piazzata",
            "Хорошее расположение ферзя",
            "Dobrze ustawiony hetman",
        ],
    ),
    (
        102,
        "Poor piece coordination",
        [
            "Má coordenação das peças",
            "Mala coordinación de las piezas",
            "Mauvaise coordination des pièces",
            "Schlechte Figurenkoordination",
            "Scarsa coordinazione dei pezzi",
            "Плохая координация фигур",
            "Słaba koordynacja figur",
        ],
    ),
    (
        104,
        "Good piece coordination",
        [
            "Boa coordenação das peças",
            "Buena coordinación de las piezas",
            "Bonne coordination des pièces",
            "Gute Figurenkoordination",
            "Buona coordinazione dei pezzi",
            "Хорошая координация фигур",
            "Dobra koordynacja figur",
        ],
    ),
    (
        106,
        "Very poorly played opening",
        [
            "Abertura muito mal jogada",
            "Apertura muy mal jugada",
            "Ouverture très mal jouée",
            "Sehr schlecht gespielte Eröffnung",
            "Apertura giocata molto male",
            "Очень плохо сыгранный дебют",
            "Bardzo źle rozegrany debiut",
        ],
    ),
    (
        108,
        "Poorly played opening",
        [
            "Abertura mal jogada",
            "Apertura mal jugada",
            "Ouverture mal jouée",
            "Schlecht gespielte Eröffnung",
            "Apertura giocata male",
            "Плохо сыгранный дебют",
            "Źle rozegrany debiut",
        ],
    ),
    (
        110,
        "Well played opening",
        [
            "Abertura bem jogada",
            "Apertura bien jugada",
            "Ouverture bien jouée",
            "Gut gespielte Eröffnung",
            "Apertura giocata bene",
            "Хорошо сыгранный дебют",
            "Dobrze rozegrany debiut",
        ],
    ),
    (
        112,
        "Very well played opening",
        [
            "Abertura muito bem jogada",
            "Apertura muy bien jugada",
            "Ouverture très bien jouée",
            "Sehr gut gespielte Eröffnung",
            "Apertura giocata molto bene",
            "Очень хорошо сыгранный дебют",
            "Bardzo dobrze rozegrany debiut",
        ],
    ),
    (
        114,
        "Very poorly played middlegame",
        [
            "Meio-jogo muito mal jogado",
            "Medio juego muy mal jugado",
            "Milieu de partie très mal joué",
            "Sehr schlecht gespieltes Mittelspiel",
            "Mediogioco giocato molto male",
            "Очень плохо сыгранный миттельшпиль",
            "Bardzo źle rozegrana gra środkowa",
        ],
    ),
    (
        116,
        "Poorly played middlegame",
        [
            "Meio-jogo mal jogado",
            "Medio juego mal jugado",
            "Milieu de partie mal joué",
            "Schlecht gespieltes Mittelspiel",
            "Mediogioco giocato male",
            "Плохо сыгранный миттельшпиль",
            "Źle rozegrana gra środkowa",
        ],
    ),
    (
        118,
        "Well played middlegame",
        [
            "Meio-jogo bem jogado",
            "Medio juego bien jugado",
            "Milieu de partie bien joué",
            "Gut gespieltes Mittelspiel",
            "Mediogioco giocato bene",
            "Хорошо сыгранный миттельшпиль",
            "Dobrze rozegrana gra środkowa",
        ],
    ),
    (
        120,
        "Very well played middlegame",
        [
            "Meio-jogo muito bem jogado",
            "Medio juego muy bien jugado",
            "Milieu de partie très bien joué",
            "Sehr gut gespieltes Mittelspiel",
            "Mediogioco giocato molto bene",
            "Очень хорошо сыгранный миттельшпиль",
            "Bardzo dobrze rozegrana gra środkowa",
        ],
    ),
    (
        122,
        "Very poorly played endgame",
        [
            "Final muito mal jogado",
            "Final muy mal jugado",
            "Finale très mal jouée",
            "Sehr schlecht gespieltes Endspiel",
            "Finale giocato molto male",
            "Очень плохо сыгранный эндшпиль",
            "Bardzo źle rozegrana końcówka",
        ],
    ),
    (
        124,
        "Poorly played endgame",
        [
            "Final mal jogado",
            "Final mal jugado",
            "Finale mal jouée",
            "Schlecht gespieltes Endspiel",
            "Finale giocato male",
            "Плохо сыгранный эндшпиль",
            "Źle rozegrana końcówka",
        ],
    ),
    (
        126,
        "Well played endgame",
        [
            "Final bem jogado",
            "Final bien jugado",
            "Finale bien jouée",
            "Gut gespieltes Endspiel",
            "Finale giocato bene",
            "Хорошо сыгранный эндшпиль",
            "Dobrze rozegrana końcówka",
        ],
    ),
    (
        128,
        "Very well played endgame",
        [
            "Final muito bem jogado",
            "Final muy bien jugado",
            "Finale très bien jouée",
            "Sehr gut gespieltes Endspiel",
            "Finale giocato molto bene",
            "Очень хорошо сыгранный эндшпиль",
            "Bardzo dobrze rozegrana końcówka",
        ],
    ),
];

/// The pieces' English names, in the order of [`PIECES`].
static PIECE_ENGLISH: [&str; 12] = [
    "White king",
    "White queen",
    "White rook",
    "White bishop",
    "White knight",
    "White pawn",
    "Black king",
    "Black queen",
    "Black rook",
    "Black bishop",
    "Black knight",
    "Black pawn",
];

/// The pieces' copies.
static PIECES: [[&str; 7]; 12] = [
    ["Rei branco", "Rey blanco", "Roi blanc", "Weißer König", "Re bianco", "Белый король", "Biały król"],
    ["Dama branca", "Dama blanca", "Dame blanche", "Weiße Dame", "Donna bianca", "Белый ферзь", "Biały hetman"],
    ["Torre branca", "Torre blanca", "Tour blanche", "Weißer Turm", "Torre bianca", "Белая ладья", "Biała wieża"],
    ["Bispo branco", "Alfil blanco", "Fou blanc", "Weißer Läufer", "Alfiere bianco", "Белый слон", "Biały goniec"],
    ["Cavalo branco", "Caballo blanco", "Cavalier blanc", "Weißer Springer", "Cavallo bianco", "Белый конь", "Biały skoczek"],
    ["Peão branco", "Peón blanco", "Pion blanc", "Weißer Bauer", "Pedone bianco", "Белая пешка", "Biały pion"],
    ["Rei preto", "Rey negro", "Roi noir", "Schwarzer König", "Re nero", "Чёрный король", "Czarny król"],
    ["Dama preta", "Dama negra", "Dame noire", "Schwarze Dame", "Donna nera", "Чёрный ферзь", "Czarny hetman"],
    ["Torre preta", "Torre negra", "Tour noire", "Schwarzer Turm", "Torre nera", "Чёрная ладья", "Czarna wieża"],
    ["Bispo preto", "Alfil negro", "Fou noir", "Schwarzer Läufer", "Alfiere nero", "Чёрный слон", "Czarny goniec"],
    ["Cavalo preto", "Caballo negro", "Cavalier noir", "Schwarzer Springer", "Cavallo nero", "Чёрный конь", "Czarny skoczek"],
    ["Peão preto", "Peón negro", "Pion noir", "Schwarzer Bauer", "Pedone nero", "Чёрная пешка", "Czarny pion"],
];

/// "To review"'s copies.
static TO_REVIEW: [&str; 7] = ["A revisar", "Por revisar", "À vérifier", "Zu prüfen", "Da rivedere", "На проверку", "Do sprawdzenia"];

/// The reasons' English names, in the order of [`Reason::ALL`].
static REASON_ENGLISH: [&str; 5] =
    ["Illegal move", "Ambiguous move", "Move out of sequence", "Not reached: an earlier move failed", "Unchecked: no position to play it from"];

/// The reasons' copies.
static REASONS: [[&str; 7]; 5] = [
    ["Lance ilegal", "Jugada ilegal", "Coup illégal", "Illegaler Zug", "Mossa illegale", "Невозможный ход", "Nielegalny ruch"],
    ["Lance ambíguo", "Jugada ambigua", "Coup ambigu", "Mehrdeutiger Zug", "Mossa ambigua", "Неоднозначный ход", "Niejednoznaczny ruch"],
    [
        "Lance fora de sequência",
        "Jugada fuera de secuencia",
        "Coup hors séquence",
        "Zug außer der Reihe",
        "Mossa fuori sequenza",
        "Ход вне очереди",
        "Ruch poza kolejnością",
    ],
    [
        "Não alcançado: um lance anterior falhou",
        "No alcanzada: falló una jugada anterior",
        "Non atteint : un coup précédent a échoué",
        "Nicht erreicht: ein früherer Zug ist gescheitert",
        "Non raggiunta: una mossa precedente non è riuscita",
        "Не достигнут: предыдущий ход не удался",
        "Nieosiągnięty: wcześniejszy ruch się nie powiódł",
    ],
    [
        "Não verificado: não há posição de onde jogá-lo",
        "Sin comprobar: no hay posición desde la que jugarla",
        "Non vérifié : aucune position d'où le jouer",
        "Nicht geprüft: keine Stellung, von der aus er gespielt werden kann",
        "Non verificata: nessuna posizione da cui giocarla",
        "Не проверен: нет позиции, из которой его сыграть",
        "Niesprawdzony: brak pozycji, z której można go zagrać",
    ],
];

#[cfg(test)]
mod tests {
    use super::*;

    /// A copy for every language a catalogue has it in, English elsewhere; every code of the NAG
    /// table has a name, either side's code the same row.
    #[test]
    fn names_fall_back_to_english_and_say_so() {
        assert_eq!(sign(16, Lang::Pt).map(|s| s.lang), Some(Lang::Pt));
        assert_eq!(sign(16, Lang::Nl).map(|s| (s.text, s.lang)), Some(("White is clearly better", Lang::En)));
        assert_eq!(piece(Color::White, PieceKind::Knight, Lang::Pt), Said { text: "Cavalo branco", lang: Lang::Pt });
        assert_eq!(piece(Color::Black, PieceKind::Knight, Lang::Cs).lang, Lang::En);
        assert_eq!(to_review(Lang::Pt), Said { text: "A revisar", lang: Lang::Pt });
        assert_eq!(reason(Reason::Illegal, Lang::Pt).text, "Lance ilegal");
        for code in 0..=300u16 {
            assert_eq!(sign(code, Lang::En).is_some(), nag::by_code(code).is_some(), "{code}");
        }
        for (english, lang, text) in copied() {
            assert!(!text.is_empty() && lang != Lang::En, "{english}");
        }
    }
}
