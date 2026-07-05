import type { JSX } from "react";
import { MdBlocks } from "../components/Md";
import { CtaLink, Shell } from "../components/Shell";
import { loadFaqGroups } from "../lib/knowledge";

export function FaqPage(): JSX.Element {
  const groups = loadFaqGroups();
  return (
    <Shell>
      <section className="section">
        <div className="container">
          <p className="overline reveal">Fragen &amp; Antworten</p>
          <h1 className="reveal">
            FAQ — <span className="gold">kurz und ehrlich beantwortet.</span>
          </h1>
          <p className="lede reveal">
            Diese Antworten kommen aus derselben Wissensbasis, mit der auch der
            Bot selbst Fragen beantwortet — eine Quelle, immer aktuell.
          </p>

          <nav className="faq-toc reveal" aria-label="FAQ-Themen">
            {groups.map((group) => (
              <a key={group.key} href={`#${group.key}`}>
                {group.title}
              </a>
            ))}
          </nav>

          {groups.map((group) => (
            <div key={group.key} className="faq-group" id={group.key}>
              <h2 className="reveal">{group.title}</h2>
              {group.intro.length > 0 && <MdBlocks blocks={group.intro} />}
              {group.items.map((item) => (
                <details key={item.question} className="panel faq-item">
                  <summary>{item.question}</summary>
                  <MdBlocks blocks={item.blocks} />
                </details>
              ))}
            </div>
          ))}

          <div className="final-inner">
            <div className="hero-actions reveal">
              <CtaLink>Alles klar — Bot reinholen</CtaLink>
            </div>
          </div>
        </div>
      </section>
    </Shell>
  );
}
